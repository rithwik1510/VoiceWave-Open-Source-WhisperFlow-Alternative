import base64
import ctypes
import json
import os
import sys
import time
import traceback
from pathlib import Path

import numpy as np
from faster_whisper import WhisperModel
try:
    import ctranslate2
except Exception:  # noqa: BLE001
    ctranslate2 = None


MODEL_CACHE = {}
ALLOWED_MODELS = {"small.en", "large-v3", "large-v3-turbo"}
GPU_CAPABILITY_CACHE = None
CUDA_RUNTIME_LIBS_READY_CACHE = None
CUDA_DLL_DIR_HANDLES = []


def _candidate_cuda_bin_dirs() -> list[str]:
    dirs: list[str] = []

    cuda_root = os.getenv("CUDA_PATH")
    if cuda_root:
        dirs.append(str(Path(cuda_root) / "bin"))
        dirs.append(str(Path(cuda_root) / "bin" / "x64"))

    venv_root = Path(sys.executable).resolve().parent.parent
    nvidia_root = venv_root / "Lib" / "site-packages" / "nvidia"
    if nvidia_root.exists():
        for entry in nvidia_root.iterdir():
            bin_dir = entry / "bin"
            if bin_dir.exists():
                dirs.append(str(bin_dir))

    deduped = []
    seen = set()
    for value in dirs:
        normalized = value.strip()
        if not normalized:
            continue
        if not Path(normalized).exists():
            continue
        if normalized in seen:
            continue
        seen.add(normalized)
        deduped.append(normalized)
    return deduped


def register_cuda_dll_dirs() -> None:
    if os.name != "nt":
        return
    if not hasattr(os, "add_dll_directory"):
        return
    if CUDA_DLL_DIR_HANDLES:
        return

    for path in _candidate_cuda_bin_dirs():
        try:
            handle = os.add_dll_directory(path)
            CUDA_DLL_DIR_HANDLES.append(handle)
        except Exception:  # noqa: BLE001
            pass


def env_flag(name: str, default_value: bool) -> bool:
    value = os.getenv(name)
    if value is None:
        return default_value
    normalized = value.strip().lower()
    if normalized in {"1", "true", "yes", "on"}:
        return True
    if normalized in {"0", "false", "no", "off"}:
        return False
    return default_value


def normalize_backend_preference(raw_backend: str | None) -> str:
    normalized = (raw_backend or "auto").strip().lower()
    if normalized in {"cpu", "cuda", "auto"}:
        return normalized
    return "auto"


def cuda_available() -> bool:
    register_cuda_dll_dirs()
    global GPU_CAPABILITY_CACHE
    if GPU_CAPABILITY_CACHE is not None:
        return GPU_CAPABILITY_CACHE
    if ctranslate2 is None:
        GPU_CAPABILITY_CACHE = False
        return GPU_CAPABILITY_CACHE
    try:
        GPU_CAPABILITY_CACHE = ctranslate2.get_cuda_device_count() > 0
    except Exception:  # noqa: BLE001
        GPU_CAPABILITY_CACHE = False
    return GPU_CAPABILITY_CACHE


def cuda_runtime_libs_ready() -> bool:
    register_cuda_dll_dirs()
    global CUDA_RUNTIME_LIBS_READY_CACHE
    if CUDA_RUNTIME_LIBS_READY_CACHE is not None:
        return CUDA_RUNTIME_LIBS_READY_CACHE

    # Avoid false positives: CTranslate2 may report CUDA support even when
    # cublas runtime DLL cannot be loaded at decode time.
    required = ["cublas64_12.dll"]
    for dll in required:
        try:
            ctypes.WinDLL(dll)
        except Exception:  # noqa: BLE001
            CUDA_RUNTIME_LIBS_READY_CACHE = False
            return CUDA_RUNTIME_LIBS_READY_CACHE

    if ctranslate2 is not None:
        try:
            supported = ctranslate2.get_supported_compute_types("cuda")
            if not supported:
                CUDA_RUNTIME_LIBS_READY_CACHE = False
                return CUDA_RUNTIME_LIBS_READY_CACHE
        except Exception:  # noqa: BLE001
            CUDA_RUNTIME_LIBS_READY_CACHE = False
            return CUDA_RUNTIME_LIBS_READY_CACHE
    CUDA_RUNTIME_LIBS_READY_CACHE = True
    return CUDA_RUNTIME_LIBS_READY_CACHE


def resolve_requested_backend(raw_backend: str | None) -> str:
    if env_flag("VOICEWAVE_FORCE_CPU", False):
        return "cpu"
    if env_flag("VOICEWAVE_FORCE_GPU", False):
        return "cuda"

    pref = normalize_backend_preference(raw_backend)
    if pref == "cpu":
        return "cpu"
    if pref == "cuda":
        return "cuda"

    if not env_flag("VOICEWAVE_AUTO_GPU", True):
        return "cpu"
    if cuda_available() and cuda_runtime_libs_ready():
        return "cuda"
    return "cpu"


def supported_compute_types(device: str) -> set[str]:
    if ctranslate2 is None:
        return set()
    try:
        return set(ctranslate2.get_supported_compute_types(device))
    except Exception:  # noqa: BLE001
        return set()


def resolve_compute_type(device: str, requested_compute_type: str) -> str:
    requested = (requested_compute_type or "int8").strip().lower()
    supported = supported_compute_types(device)
    if not supported:
        return requested
    if requested in supported:
        return requested

    if device == "cuda":
        preference = [
            "int8_float16",
            "float16",
            "int8",
            "float32",
            "int8_float32",
        ]
    else:
        preference = [
            "int8",
            "int8_float32",
            "float32",
        ]
    for candidate in preference:
        if candidate in supported:
            return candidate
    return requested


def load_model(model_id: str, device: str, compute_type: str):
    key = (model_id, device, compute_type)
    cached = MODEL_CACHE.get(key)
    if cached is not None:
        return cached, True, 0

    started = time.perf_counter()
    model = WhisperModel(model_id, device=device, compute_type=compute_type)
    load_ms = int((time.perf_counter() - started) * 1000)
    MODEL_CACHE[key] = model
    return model, False, load_ms


def transcribe(req: dict) -> dict:
    request_id = req.get("id")
    audio_path = req.get("audioPath")
    audio_pcm16_b64 = req.get("audioPcm16B64")
    sample_rate_hz = int(req.get("sampleRateHz", 16_000))
    model_id = req.get("modelId")
    compute_type = req.get("computeType", "int8")
    backend_preference = req.get("backendPreference", "auto")
    allow_backend_fallback = bool(req.get("allowBackendFallback", True))
    beam_size = int(req.get("beamSize", 2))
    best_of = int(req.get("bestOf", 1))
    language = req.get("language", "en")
    vad_filter = bool(req.get("vadFilter", True))
    condition_on_previous_text = bool(req.get("conditionOnPreviousText", False))
    without_timestamps = bool(req.get("withoutTimestamps", False))
    initial_prompt = req.get("initialPrompt")
    temperature = req.get("temperature")
    no_speech_threshold = req.get("noSpeechThreshold")
    log_prob_threshold = req.get("logProbThreshold")
    compression_ratio_threshold = req.get("compressionRatioThreshold")
    if isinstance(initial_prompt, str):
        initial_prompt = initial_prompt.strip() or None
    else:
        initial_prompt = None

    if not model_id:
        return {
            "id": request_id,
            "ok": False,
            "error": "Model ID is required.",
        }
    if model_id not in ALLOWED_MODELS:
        return {
            "id": request_id,
            "ok": False,
            "error": f"Unsupported model ID: {model_id}. Allowed: small.en, large-v3, large-v3-turbo",
        }
    if sample_rate_hz != 16_000:
        return {
            "id": request_id,
            "ok": False,
            "error": f"Unsupported sample rate: {sample_rate_hz}. Expected 16000 Hz.",
        }

    audio_input = None
    if isinstance(audio_pcm16_b64, str) and audio_pcm16_b64.strip():
        try:
            pcm_bytes = base64.b64decode(audio_pcm16_b64)
        except Exception as exc:  # noqa: BLE001
            return {
                "id": request_id,
                "ok": False,
                "error": f"Invalid in-memory PCM payload: {exc}",
            }
        if len(pcm_bytes) % 2 != 0:
            return {
                "id": request_id,
                "ok": False,
                "error": "Invalid in-memory PCM payload length.",
            }
        audio_input = np.frombuffer(pcm_bytes, dtype=np.int16).astype(np.float32) / 32768.0
    elif audio_path and Path(audio_path).exists():
        audio_input = audio_path
    else:
        return {
            "id": request_id,
            "ok": False,
            "error": "Audio payload is missing. Provide audioPcm16B64 or a valid audioPath.",
        }

    backend_requested = resolve_requested_backend(backend_preference)
    backend_used = backend_requested
    backend_fallback = False
    compute_type_requested = str(compute_type)
    compute_type_used = resolve_compute_type(backend_used, compute_type_requested)

    transcribe_kwargs = {
        "beam_size": beam_size,
        "best_of": best_of,
        "language": language,
        "vad_filter": vad_filter,
        "condition_on_previous_text": condition_on_previous_text,
        "without_timestamps": without_timestamps,
        "initial_prompt": initial_prompt,
    }
    if temperature is not None:
        transcribe_kwargs["temperature"] = float(temperature)
    if no_speech_threshold is not None:
        transcribe_kwargs["no_speech_threshold"] = float(no_speech_threshold)
    if log_prob_threshold is not None:
        transcribe_kwargs["log_prob_threshold"] = float(log_prob_threshold)
    if compression_ratio_threshold is not None:
        transcribe_kwargs["compression_ratio_threshold"] = float(
            compression_ratio_threshold
        )

    decode_started = time.perf_counter()
    runtime_cache_hit = False
    model_init_ms = 0
    try:
        model, runtime_cache_hit, model_init_ms = load_model(
            model_id, backend_used, compute_type_used
        )
        segments, _info = model.transcribe(audio_input, **transcribe_kwargs)
        segments = list(segments)
    except Exception as first_err:  # noqa: BLE001
        if backend_used != "cuda" or not allow_backend_fallback:
            return {
                "id": request_id,
                "ok": False,
                "error": f"Transcription failed ({backend_used}/{compute_type_used}): {first_err}",
            }

        backend_fallback = True
        backend_used = "cpu"
        compute_type_used = resolve_compute_type("cpu", compute_type_requested)
        try:
            fallback_model, fallback_cache_hit, fallback_model_init_ms = load_model(
                model_id, backend_used, compute_type_used
            )
            model_init_ms += fallback_model_init_ms
            runtime_cache_hit = runtime_cache_hit and fallback_cache_hit
            segments, _info = fallback_model.transcribe(audio_input, **transcribe_kwargs)
            segments = list(segments)
        except Exception as fallback_err:  # noqa: BLE001
            return {
                "id": request_id,
                "ok": False,
                "error": (
                    f"Transcription failed ({backend_requested}/{compute_type_requested}) "
                    f"with fallback ({backend_used}/{compute_type_used}): {fallback_err}"
                ),
            }

    decode_ms = int((time.perf_counter() - decode_started) * 1000)

    parts = []
    avg_logprobs = []
    no_speech_probs = []
    compression_ratios = []
    for segment in segments:
        text = (segment.text or "").strip()
        if text:
            parts.append(text)
        avg_logprobs.append(float(getattr(segment, "avg_logprob", 0.0)))
        no_speech_probs.append(float(getattr(segment, "no_speech_prob", 0.0)))
        compression_ratios.append(float(getattr(segment, "compression_ratio", 0.0)))

    mean_avg_logprob = (
        sum(avg_logprobs) / len(avg_logprobs) if avg_logprobs else 0.0
    )
    mean_no_speech_prob = (
        sum(no_speech_probs) / len(no_speech_probs) if no_speech_probs else 0.0
    )
    mean_compression_ratio = (
        sum(compression_ratios) / len(compression_ratios) if compression_ratios else 0.0
    )

    return {
        "id": request_id,
        "ok": True,
        "text": " ".join(parts).strip(),
        "modelInitMs": model_init_ms,
        "decodeComputeMs": decode_ms,
        "runtimeCacheHit": runtime_cache_hit,
        "segmentCount": len(segments),
        "avgLogProb": mean_avg_logprob,
        "noSpeechProb": mean_no_speech_prob,
        "compressionRatio": mean_compression_ratio,
        "backendRequested": backend_requested,
        "backendUsed": backend_used,
        "backendFallback": backend_fallback,
        "computeTypeRequested": compute_type_requested,
        "computeTypeUsed": compute_type_used,
    }


def kernel_warmup_decode(model) -> int:
    """Tiny throwaway decode to force CUDA kernel compilation / clock ramp.

    The first transcribe after a model load pays 1-2.5 s of one-time GPU
    setup on top of the load itself (observed in production diagnostics).
    Running a half-second silent decode right after load moves that cost
    into the background prefetch instead of the user's first dictation.
    Returns the warmup duration in ms; failures are swallowed (warmup is
    opportunistic, never load-bearing).
    """
    started = time.perf_counter()
    try:
        audio = np.zeros(8_000, dtype=np.float32)  # 0.5 s of silence @16 kHz
        segments, _info = model.transcribe(
            audio,
            beam_size=1,
            best_of=1,
            language="en",
            vad_filter=False,
            condition_on_previous_text=False,
            without_timestamps=True,
            temperature=0.0,
        )
        list(segments)
    except Exception:  # noqa: BLE001
        pass
    return int((time.perf_counter() - started) * 1000)


def warmup(req: dict) -> dict:
    """Warm the GPU for an imminent decode (called when dictation starts)."""
    request_id = req.get("id")
    model_id = req.get("modelId")
    compute_type = req.get("computeType", "int8")
    backend_preference = req.get("backendPreference", "auto")
    if not model_id:
        return {"id": request_id, "ok": False, "error": "Model ID is required."}
    if model_id not in ALLOWED_MODELS:
        return {
            "id": request_id,
            "ok": False,
            "error": f"Unsupported model ID: {model_id}. Allowed: small.en, large-v3, large-v3-turbo",
        }
    backend_used = resolve_requested_backend(backend_preference)
    compute_type_used = resolve_compute_type(backend_used, str(compute_type))
    try:
        model, runtime_cache_hit, model_init_ms = load_model(
            model_id, backend_used, compute_type_used
        )
    except Exception as err:  # noqa: BLE001
        return {
            "id": request_id,
            "ok": False,
            "error": f"Warmup failed ({backend_used}/{compute_type_used}): {err}",
        }
    warmup_ms = kernel_warmup_decode(model)
    return {
        "id": request_id,
        "ok": True,
        "modelInitMs": model_init_ms,
        "runtimeCacheHit": runtime_cache_hit,
        "warmupMs": warmup_ms,
        "backendUsed": backend_used,
    }


def prefetch(req: dict) -> dict:
    request_id = req.get("id")
    model_id = req.get("modelId")
    compute_type = req.get("computeType", "int8")
    backend_preference = req.get("backendPreference", "auto")
    allow_backend_fallback = bool(req.get("allowBackendFallback", True))
    if not model_id:
        return {"id": request_id, "ok": False, "error": "Model ID is required."}
    if model_id not in ALLOWED_MODELS:
        return {
            "id": request_id,
            "ok": False,
            "error": f"Unsupported model ID: {model_id}. Allowed: small.en, large-v3, large-v3-turbo",
        }
    backend_requested = resolve_requested_backend(backend_preference)
    backend_used = backend_requested
    backend_fallback = False
    compute_type_requested = str(compute_type)
    compute_type_used = resolve_compute_type(backend_used, compute_type_requested)
    runtime_cache_hit = False
    model_init_ms = 0
    warmup_ms = 0
    try:
        model, runtime_cache_hit, model_init_ms = load_model(
            model_id, backend_used, compute_type_used
        )
    except Exception as first_err:  # noqa: BLE001
        if backend_used != "cuda" or not allow_backend_fallback:
            return {
                "id": request_id,
                "ok": False,
                "error": f"Prefetch failed ({backend_used}/{compute_type_used}): {first_err}",
            }
        backend_fallback = True
        backend_used = "cpu"
        compute_type_used = resolve_compute_type("cpu", compute_type_requested)
        try:
            model, fallback_cache_hit, fallback_model_init_ms = load_model(
                model_id, backend_used, compute_type_used
            )
            model_init_ms += fallback_model_init_ms
            runtime_cache_hit = runtime_cache_hit and fallback_cache_hit
        except Exception as fallback_err:  # noqa: BLE001
            return {
                "id": request_id,
                "ok": False,
                "error": (
                    f"Prefetch failed ({backend_requested}/{compute_type_requested}) "
                    f"with fallback ({backend_used}/{compute_type_used}): {fallback_err}"
                ),
            }
    if not runtime_cache_hit:
        # Fresh load: pay the one-time kernel compilation here, in the
        # background, instead of on the user's first dictation.
        warmup_ms = kernel_warmup_decode(model)
    return {
        "id": request_id,
        "ok": True,
        "modelInitMs": model_init_ms,
        "warmupMs": warmup_ms,
        "runtimeCacheHit": runtime_cache_hit,
        "backendRequested": backend_requested,
        "backendUsed": backend_used,
        "backendFallback": backend_fallback,
        "computeTypeRequested": compute_type_requested,
        "computeTypeUsed": compute_type_used,
    }


def main() -> int:
    print(json.dumps({"ready": True}), flush=True)
    for line in sys.stdin:
        raw = line.strip()
        if not raw:
            continue
        try:
            req = json.loads(raw)
            command = req.get("command", "transcribe")
            if command == "shutdown":
                print(json.dumps({"ok": True, "shutdown": True}), flush=True)
                return 0
            if command == "prefetch":
                print(json.dumps(prefetch(req)), flush=True)
                continue
            if command == "warmup":
                print(json.dumps(warmup(req)), flush=True)
                continue
            if command != "transcribe":
                print(
                    json.dumps(
                        {
                            "id": req.get("id"),
                            "ok": False,
                            "error": f"Unsupported command: {command}",
                        }
                    ),
                    flush=True,
                )
                continue

            response = transcribe(req)
            print(json.dumps(response), flush=True)
        except Exception as exc:  # noqa: BLE001
            print(
                json.dumps(
                    {
                        "id": None,
                        "ok": False,
                        "error": f"Worker exception: {exc}",
                        "traceback": traceback.format_exc(),
                    }
                ),
                flush=True,
            )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
