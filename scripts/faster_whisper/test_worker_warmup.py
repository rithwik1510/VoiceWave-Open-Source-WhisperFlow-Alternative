import importlib.util
import sys
import types
import unittest
from pathlib import Path
from unittest.mock import Mock, patch


ROOT = Path(__file__).resolve().parents[2]
WORKER_PATHS = (
    ROOT / "scripts" / "faster_whisper" / "worker.py",
    ROOT / "src-tauri" / "windows" / "faster-whisper" / "worker.py",
)


def load_worker(path: Path, suffix: str):
    fake_numpy = types.ModuleType("numpy")
    fake_numpy.float32 = float
    fake_numpy.int16 = int
    fake_numpy.zeros = Mock(return_value=[])
    fake_numpy.frombuffer = Mock(return_value=[])
    fake_faster_whisper = types.ModuleType("faster_whisper")
    fake_faster_whisper.WhisperModel = Mock
    fake_ctranslate2 = types.ModuleType("ctranslate2")
    fake_ctranslate2.get_cuda_device_count = lambda: 0
    fake_ctranslate2.get_supported_compute_types = lambda _device: {"int8"}

    module_name = f"voicewave_worker_{suffix}"
    spec = importlib.util.spec_from_file_location(module_name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"Could not load worker at {path}")
    module = importlib.util.module_from_spec(spec)
    with patch.dict(
        sys.modules,
        {
            "numpy": fake_numpy,
            "faster_whisper": fake_faster_whisper,
            "ctranslate2": fake_ctranslate2,
        },
    ):
        spec.loader.exec_module(module)
    return module


class WorkerWarmupTests(unittest.TestCase):
    def test_cpu_warmup_loads_model_without_dummy_decode(self):
        for index, path in enumerate(WORKER_PATHS):
            with self.subTest(path=path):
                worker = load_worker(path, f"cpu_warmup_{index}")
                model = object()
                worker.resolve_requested_backend = Mock(return_value="cpu")
                worker.resolve_compute_type = Mock(return_value="int8")
                worker.load_model = Mock(return_value=(model, False, 17))
                worker.kernel_warmup_decode = Mock(return_value=41)

                response = worker.warmup(
                    {
                        "id": "warm-cpu",
                        "modelId": "small.en",
                        "backendPreference": "cpu",
                    }
                )

                worker.load_model.assert_called_once_with("small.en", "cpu", "int8", False)
                worker.kernel_warmup_decode.assert_not_called()
                self.assertTrue(response["ok"])
                self.assertEqual(response["modelInitMs"], 17)
                self.assertEqual(response["warmupMs"], 0)
                self.assertEqual(response["backendUsed"], "cpu")

    def test_cuda_warmup_still_runs_dummy_decode(self):
        for index, path in enumerate(WORKER_PATHS):
            with self.subTest(path=path):
                worker = load_worker(path, f"cuda_warmup_{index}")
                model = object()
                worker.resolve_requested_backend = Mock(return_value="cuda")
                worker.resolve_compute_type = Mock(return_value="int8_float16")
                worker.load_model = Mock(return_value=(model, True, 0))
                worker.kernel_warmup_decode = Mock(return_value=29)

                response = worker.warmup(
                    {
                        "id": "warm-cuda",
                        "modelId": "small.en",
                        "backendPreference": "cuda",
                    }
                )

                worker.kernel_warmup_decode.assert_called_once_with(model)
                self.assertTrue(response["ok"])
                self.assertEqual(response["warmupMs"], 29)
                self.assertEqual(response["backendUsed"], "cuda")

    def test_cpu_prefetch_loads_model_without_dummy_decode(self):
        for index, path in enumerate(WORKER_PATHS):
            with self.subTest(path=path):
                worker = load_worker(path, f"cpu_prefetch_{index}")
                model = object()
                worker.resolve_requested_backend = Mock(return_value="cpu")
                worker.resolve_compute_type = Mock(return_value="int8")
                worker.load_model = Mock(return_value=(model, False, 13))
                worker.kernel_warmup_decode = Mock(return_value=37)

                response = worker.prefetch(
                    {
                        "id": "prefetch-cpu",
                        "modelId": "small.en",
                        "backendPreference": "cpu",
                    }
                )

                worker.load_model.assert_called_once_with("small.en", "cpu", "int8", False)
                worker.kernel_warmup_decode.assert_not_called()
                self.assertTrue(response["ok"])
                self.assertFalse(response["runtimeCacheHit"])
                self.assertEqual(response["modelInitMs"], 13)
                self.assertEqual(response["warmupMs"], 0)
                self.assertEqual(response["backendUsed"], "cpu")

    def test_fresh_cuda_prefetch_still_runs_dummy_decode(self):
        for index, path in enumerate(WORKER_PATHS):
            with self.subTest(path=path):
                worker = load_worker(path, f"cuda_prefetch_{index}")
                model = object()
                worker.resolve_requested_backend = Mock(return_value="cuda")
                worker.resolve_compute_type = Mock(return_value="int8_float16")
                worker.load_model = Mock(return_value=(model, False, 23))
                worker.kernel_warmup_decode = Mock(return_value=31)

                response = worker.prefetch(
                    {
                        "id": "prefetch-cuda",
                        "modelId": "small.en",
                        "backendPreference": "cuda",
                    }
                )

                worker.kernel_warmup_decode.assert_called_once_with(model)
                self.assertTrue(response["ok"])
                self.assertEqual(response["warmupMs"], 31)
                self.assertEqual(response["backendUsed"], "cuda")

    def test_cuda_prefetch_fallback_to_cpu_skips_dummy_decode(self):
        for index, path in enumerate(WORKER_PATHS):
            with self.subTest(path=path):
                worker = load_worker(path, f"fallback_prefetch_{index}")
                cpu_model = object()
                worker.resolve_requested_backend = Mock(return_value="cuda")
                worker.resolve_compute_type = Mock(
                    side_effect=lambda device, _requested: (
                        "int8_float16" if device == "cuda" else "int8"
                    )
                )
                worker.load_model = Mock(
                    side_effect=[RuntimeError("CUDA unavailable"), (cpu_model, False, 19)]
                )
                worker.kernel_warmup_decode = Mock(return_value=43)

                response = worker.prefetch(
                    {
                        "id": "prefetch-fallback",
                        "modelId": "small.en",
                        "backendPreference": "cuda",
                        "allowBackendFallback": True,
                    }
                )

                worker.kernel_warmup_decode.assert_not_called()
                self.assertTrue(response["ok"])
                self.assertTrue(response["backendFallback"])
                self.assertEqual(response["backendRequested"], "cuda")
                self.assertEqual(response["backendUsed"], "cpu")
                self.assertEqual(response["computeTypeUsed"], "int8")
                self.assertEqual(response["modelInitMs"], 19)
                self.assertEqual(response["warmupMs"], 0)


if __name__ == "__main__":
    unittest.main()
