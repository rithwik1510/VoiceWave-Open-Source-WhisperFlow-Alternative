// voicewave-asr: VoiceWave's whisper.cpp speech worker (plan 019).
//
// Built as two executables from this one file (see CMakeLists.txt):
//   voicewave-asr-vk.exe   Vulkan + CPU (VOICEWAVE_ASR_VULKAN defined)
//   voicewave-asr-cpu.exe  CPU only, no Vulkan code or import at all
//
// Protocol: JSON lines. Requests on stdin, responses on stdout, all
// whisper/ggml logging on stderr. stdout never carries anything but JSON: fd 1
// is redirected to stderr at startup and responses go through a private dup.
//
//   on start   -> {"ready":true,"protocol":1,"variant":"vk"|"cpu","whisperVersion":"1.9.5"}
//   {"id":..,"command":"list_devices"}
//   {"id":..,"command":"load","modelPath":..,"device":"gpu"|"cpu","luid"?:..,"ggmlIndex"?:n,
//                             "flashAttention"?:bool,"threads"?:n,"warmup"?:bool}
//   {"id":..,"command":"transcribe","audioPcm16B64":..,"sampleRateHz"?:16000,"beamSize"?:5,
//                                   "language"?:"en","initialPrompt"?:..}
//   {"id":..,"command":"cancel","targetId"?:..}   abandons the running transcribe
//   {"id":..,"command":"shutdown"}
//
// Every response echoes the request id. Failures are
// {"id":..,"ok":false,"errorClass":"artifact"|"resource"|"backend"|"protocol","error":..}.
// A cancelled transcribe answers {"id":..,"ok":false,"cancelled":true,...} (no errorClass).
// `voicewave-asr-*.exe --list-devices` prints the list_devices response (no id)
// and exits; it needs no model.

#include <windows.h>
#include <fcntl.h>
#include <io.h>

#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <deque>
#include <iostream>
#include <mutex>
#include <new>
#include <regex>
#include <string>
#include <thread>
#include <vector>

#include "ggml-backend.h"
#include "json.hpp"
#include "whisper.h"

#ifdef VOICEWAVE_ASR_VULKAN
#include <vulkan/vulkan.h>
#endif

using json = nlohmann::json;

namespace {

constexpr int kProtocolVersion = 1;
constexpr int kSampleRate = 16000;

// ---------------------------------------------------------------- output ---

FILE *g_out = nullptr;  // private dup of the original stdout
std::mutex g_out_mu;

void emit(const json &j) {
    // `replace`: a token split inside a UTF-8 sequence must not kill the worker.
    const std::string s = j.dump(-1, ' ', false, json::error_handler_t::replace);
    std::lock_guard<std::mutex> lock(g_out_mu);
    fwrite(s.data(), 1, s.size(), g_out);
    fputc('\n', g_out);
    fflush(g_out);
}

json fail(const json &id, const char *error_class, const std::string &message) {
    return {{"id", id}, {"ok", false}, {"errorClass", error_class}, {"error", message}};
}

[[noreturn]] void exit_now(int code) {
    // Skip DLL/driver teardown: a wedged GPU driver must not stall our exit.
    TerminateProcess(GetCurrentProcess(), (UINT) code);
    std::abort();
}

// --------------------------------------------------------------- logging ---

std::mutex g_log_mu;
std::string *g_log_capture = nullptr;  // set while a load/transcribe runs

void log_callback(enum ggml_log_level, const char *text, void *) {
    if (!text) return;
    fputs(text, stderr);
    std::lock_guard<std::mutex> lock(g_log_mu);
    if (g_log_capture) g_log_capture->append(text);
}

// Collects whisper/ggml log output for the lifetime of one request, used to
// classify failures and to see which backend whisper actually initialized.
struct LogCapture {
    std::string text;
    LogCapture() { std::lock_guard<std::mutex> l(g_log_mu); g_log_capture = &text; }
    ~LogCapture() { std::lock_guard<std::mutex> l(g_log_mu); g_log_capture = nullptr; }
    std::string snapshot() { std::lock_guard<std::mutex> l(g_log_mu); return text; }
};

bool contains_any(const std::string &haystack, std::initializer_list<const char *> needles) {
    std::string lower(haystack);
    std::transform(lower.begin(), lower.end(), lower.begin(), [](unsigned char c) { return (char) tolower(c); });
    for (const char *n : needles) {
        if (lower.find(n) != std::string::npos) return true;
    }
    return false;
}

// Best-effort error class from whisper/ggml log text or an exception message.
const char *classify(const std::string &text, const char *fallback) {
    if (contains_any(text, {"bad magic", "unknown tensor", "wrong size in model", "wrong shape in model",
                            "failed to open model", "model file"})) {
        return "artifact";
    }
    if (contains_any(text, {"out of memory", "outofdevicememory", "outofhostmemory", "out_of_device_memory",
                            "out_of_host_memory", "failed to allocate", "bad allocation", "bad_alloc"})) {
        return "resource";
    }
    if (contains_any(text, {"devicelost", "device lost", "device_lost"})) return "backend";
    return fallback;
}

std::string last_error_line(const std::string &log) {
    size_t end = log.size();
    while (end > 0) {
        size_t start = log.rfind('\n', end - 1);
        start = start == std::string::npos ? 0 : start + 1;
        std::string line = log.substr(start, end - start);
        if (contains_any(line, {"error", "failed", "invalid", "wrong"})) {
            while (!line.empty() && (line.back() == '\n' || line.back() == '\r')) line.pop_back();
            return line;
        }
        if (start == 0) break;
        end = start - 1;
    }
    return "";
}

// ---------------------------------------------------------------- utils ---

double now_ms() {
    using namespace std::chrono;
    return duration<double, std::milli>(steady_clock::now().time_since_epoch()).count();
}

std::wstring utf8_to_wide(const std::string &s) {
    if (s.empty()) return {};
    int n = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, s.data(), (int) s.size(), nullptr, 0);
    if (n <= 0) return {};
    std::wstring w((size_t) n, L'\0');
    MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, s.data(), (int) s.size(), w.data(), n);
    return w;
}

bool base64_decode(const std::string &in, std::vector<uint8_t> &out) {
    static int8_t table[256];
    static bool init = false;
    if (!init) {
        std::fill(std::begin(table), std::end(table), (int8_t) -1);
        const char *chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        for (int i = 0; i < 64; i++) table[(unsigned char) chars[i]] = (int8_t) i;
        init = true;
    }
    out.clear();
    out.reserve(in.size() / 4 * 3);
    uint32_t acc = 0;
    int bits = 0;
    for (unsigned char c : in) {
        if (c == '=') break;
        if (c == '\r' || c == '\n' || c == ' ') continue;
        int8_t v = table[c];
        if (v < 0) return false;
        acc = (acc << 6) | (uint32_t) v;
        bits += 6;
        if (bits >= 8) {
            bits -= 8;
            out.push_back((uint8_t) ((acc >> bits) & 0xFF));
        }
    }
    return true;
}

std::string hex(const uint8_t *p, size_t n) {
    static const char *d = "0123456789abcdef";
    std::string s;
    for (size_t i = 0; i < n; i++) {
        s += d[p[i] >> 4];
        s += d[p[i] & 15];
    }
    return s;
}

// --------------------------------------------------------------- devices ---
//
// ggml <-> physical device mapping. Vulkan device order and ggml's own device
// filtering differ between versions and machines, so the worker never guesses:
// it enumerates the physical devices itself, decides which ones ggml may use,
// and passes exactly that list to ggml via GGML_VK_VISIBLE_DEVICES before ggml
// initializes. ggml device k is then, by construction, the k-th entry of that
// list. load re-checks name + PCI bus id against ggml before decoding.

struct Device {
    json info;
    int ggml_index = -1;  // -1: ggml will not see this device
    std::string name;
    std::string luid;  // empty when the driver reports no LUID
    std::string pci_bus_id;
};

struct DeviceTable {
    bool enumerated = false;
    bool vulkan_ok = false;  // loader present, >= 1.2, instance created
    std::string vulkan_error;
    std::vector<Device> devices;
};

DeviceTable g_devices;

#ifdef VOICEWAVE_ASR_VULKAN
const char *device_type_name(VkPhysicalDeviceType t) {
    switch (t) {
        case VK_PHYSICAL_DEVICE_TYPE_DISCRETE_GPU: return "discrete";
        case VK_PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU: return "integrated";
        case VK_PHYSICAL_DEVICE_TYPE_VIRTUAL_GPU: return "virtual";
        case VK_PHYSICAL_DEVICE_TYPE_CPU: return "cpu";
        default: return "other";
    }
}

std::string version_string(uint32_t v) {
    return std::to_string(VK_API_VERSION_MAJOR(v)) + "." + std::to_string(VK_API_VERSION_MINOR(v)) + "." +
           std::to_string(VK_API_VERSION_PATCH(v));
}

void enumerate_vulkan(DeviceTable &t) {
    // vulkan-1.dll is delay-loaded; probe it before touching any Vulkan symbol.
    HMODULE loader = LoadLibraryExW(L"vulkan-1.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!loader) {
        t.vulkan_error = "vulkan-1.dll not found (no Vulkan driver installed)";
        return;
    }
    auto enumerate_version = (PFN_vkEnumerateInstanceVersion) GetProcAddress(loader, "vkEnumerateInstanceVersion");
    uint32_t instance_version = VK_API_VERSION_1_0;
    if (enumerate_version) enumerate_version(&instance_version);
    if (instance_version < VK_API_VERSION_1_2) {
        // ggml-vulkan refuses to initialize below 1.2.
        t.vulkan_error = "Vulkan loader " + version_string(instance_version) + " is older than 1.2";
        return;
    }

    VkApplicationInfo app{VK_STRUCTURE_TYPE_APPLICATION_INFO};
    app.pApplicationName = "voicewave-asr";
    app.apiVersion = VK_API_VERSION_1_2;
    VkInstanceCreateInfo create{VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO};
    create.pApplicationInfo = &app;
    VkInstance instance = VK_NULL_HANDLE;
    VkResult r = vkCreateInstance(&create, nullptr, &instance);
    if (r != VK_SUCCESS) {
        t.vulkan_error = "vkCreateInstance failed (" + std::to_string((int) r) + ")";
        return;
    }
    t.vulkan_ok = true;

    uint32_t count = 0;
    vkEnumeratePhysicalDevices(instance, &count, nullptr);
    std::vector<VkPhysicalDevice> physical(count);
    vkEnumeratePhysicalDevices(instance, &count, physical.data());

    std::vector<std::string> visible_uuids;
    std::string visible_list;
    for (uint32_t i = 0; i < count; i++) {
        VkPhysicalDevice pd = physical[i];
        VkPhysicalDeviceProperties base{};
        vkGetPhysicalDeviceProperties(pd, &base);

        uint32_t ext_count = 0;
        vkEnumerateDeviceExtensionProperties(pd, nullptr, &ext_count, nullptr);
        std::vector<VkExtensionProperties> exts(ext_count);
        vkEnumerateDeviceExtensionProperties(pd, nullptr, &ext_count, exts.data());
        auto has_ext = [&](const char *name) {
            for (auto &e : exts) {
                if (strcmp(e.extensionName, name) == 0) return true;
            }
            return false;
        };

        Device dev;
        dev.name = base.deviceName;
        json info = {
            {"vkIndex", i},
            {"name", dev.name},
            {"vendorId", base.vendorID},
            {"deviceId", base.deviceID},
            {"type", device_type_name(base.deviceType)},
            {"apiVersion", version_string(base.apiVersion)},
            {"driverVersion", base.driverVersion},
        };

        bool fp16_storage = false;
        std::string uuid;
        if (base.apiVersion >= VK_API_VERSION_1_1) {
            VkPhysicalDeviceIDProperties id{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ID_PROPERTIES};
            VkPhysicalDeviceDriverProperties driver{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_DRIVER_PROPERTIES};
            VkPhysicalDevicePCIBusInfoPropertiesEXT pci{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PCI_BUS_INFO_PROPERTIES_EXT};
            VkPhysicalDeviceProperties2 props2{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2};
            props2.pNext = &id;
            void **tail = &id.pNext;
            const bool has_driver = base.apiVersion >= VK_API_VERSION_1_2 || has_ext("VK_KHR_driver_properties");
            const bool has_pci = has_ext("VK_EXT_pci_bus_info");
            if (has_driver) { *tail = &driver; tail = &driver.pNext; }
            if (has_pci) { *tail = &pci; tail = &pci.pNext; }
            vkGetPhysicalDeviceProperties2(pd, &props2);

            uuid = hex(id.deviceUUID, VK_UUID_SIZE);
            info["uuid"] = uuid;
            if (id.deviceLUIDValid) {
                // Same 64-bit value as a DXGI/Win32 LUID: (HighPart << 32) | LowPart.
                uint64_t luid = 0;
                memcpy(&luid, id.deviceLUID, sizeof(luid));
                char buf[17];
                snprintf(buf, sizeof(buf), "%016llx", (unsigned long long) luid);
                dev.luid = buf;
                info["luid"] = dev.luid;
            } else {
                info["luid"] = nullptr;
            }
            if (has_driver) {
                info["driverName"] = driver.driverName;
                info["driverInfo"] = driver.driverInfo;
            }
            if (has_pci) {
                // Same format ggml uses for ggml_backend_dev_props.device_id.
                char buf[16];
                snprintf(buf, sizeof(buf), "%04x:%02x:%02x.%x", pci.pciDomain, pci.pciBus, pci.pciDevice,
                         (unsigned) (uint8_t) pci.pciFunction);
                dev.pci_bus_id = buf;
                info["pciBusId"] = dev.pci_bus_id;
            }

            VkPhysicalDevice16BitStorageFeatures storage16{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_16BIT_STORAGE_FEATURES};
            VkPhysicalDeviceFeatures2 features2{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2};
            features2.pNext = &storage16;
            vkGetPhysicalDeviceFeatures2(pd, &features2);
            fp16_storage = storage16.storageBuffer16BitAccess == VK_TRUE;
        }

        VkPhysicalDeviceMemoryProperties mem{};
        vkGetPhysicalDeviceMemoryProperties(pd, &mem);
        uint64_t device_local = 0;
        for (uint32_t h = 0; h < mem.memoryHeapCount; h++) {
            if (mem.memoryHeaps[h].flags & VK_MEMORY_HEAP_DEVICE_LOCAL_BIT) device_local += mem.memoryHeaps[h].size;
        }
        info["memoryBytes"] = device_local;  // device-local heaps (on an APU: the BIOS carve-out)

        // ggml's own rules (ggml_vk_instance_init + ggml_vk_device_is_supported):
        // discrete/integrated, Vulkan 1.2+, 16-bit storage buffers, one entry per UUID.
        std::string reason;
        if (base.deviceType != VK_PHYSICAL_DEVICE_TYPE_DISCRETE_GPU &&
            base.deviceType != VK_PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU) {
            reason = "not a discrete or integrated GPU";
        } else if (base.apiVersion < VK_API_VERSION_1_2) {
            reason = "Vulkan " + version_string(base.apiVersion) + " is older than 1.2";
        } else if (!fp16_storage) {
            reason = "no 16-bit storage buffer support";
        } else if (std::find(visible_uuids.begin(), visible_uuids.end(), uuid) != visible_uuids.end()) {
            reason = "duplicate of another device (same UUID)";
        }
        if (reason.empty()) {
            dev.ggml_index = (int) visible_uuids.size();
            visible_uuids.push_back(uuid);
            if (!visible_list.empty()) visible_list += ",";
            visible_list += std::to_string(i);
            info["ggmlIndex"] = dev.ggml_index;
        } else {
            info["ggmlIndex"] = nullptr;
            info["unusableReason"] = reason;
        }
        dev.info = std::move(info);
        t.devices.push_back(std::move(dev));
    }
    vkDestroyInstance(instance, nullptr);

    // Must happen before ggml's backend registry initializes (first ggml call).
    // With no usable device we leave ggml to its defaults; GPU loads are refused.
    if (!visible_list.empty()) _putenv_s("GGML_VK_VISIBLE_DEVICES", visible_list.c_str());
}
#endif

void ensure_devices() {
    if (g_devices.enumerated) return;
    g_devices.enumerated = true;
#ifdef VOICEWAVE_ASR_VULKAN
    enumerate_vulkan(g_devices);
#endif
}

json device_list_response(const json &id) {
    ensure_devices();
    json devices = json::array();
    for (auto &d : g_devices.devices) devices.push_back(d.info);
    json out = {{"ok", true}, {"variant", VOICEWAVE_ASR_VARIANT}, {"devices", devices}};
#ifdef VOICEWAVE_ASR_VULKAN
    out["vulkanAvailable"] = g_devices.vulkan_ok;
    if (!g_devices.vulkan_error.empty()) out["vulkanError"] = g_devices.vulkan_error;
#else
    out["vulkanAvailable"] = false;
#endif
    if (!id.is_null()) out["id"] = id;
    return out;
}

// ----------------------------------------------------------------- model ---

struct Loaded {
    whisper_context *ctx = nullptr;
    std::string backend;  // "vulkan" | "cpu": what whisper actually initialized
    std::string device_name;
    std::string luid;
    int ggml_index = -1;
    int threads = 4;
};

Loaded g_model;

std::atomic<bool> g_cancel{false};
std::mutex g_running_mu;
json g_running_id;  // id of the transcribe in flight, null when idle

int default_threads() {
    unsigned hc = std::thread::hardware_concurrency();
    return (int) std::clamp(hc / 2, 1u, 8u);
}

// whisper.cpp opens model paths with narrow fopen, which breaks on non-ASCII
// Windows user folders. Feed it through a loader backed by _wfopen instead.
size_t file_read(void *ctx, void *out, size_t n) { return fread(out, 1, n, (FILE *) ctx); }
bool file_eof(void *ctx) { return feof((FILE *) ctx) != 0; }
void file_close(void *ctx) { fclose((FILE *) ctx); }

void unload() {
    if (g_model.ctx) whisper_free(g_model.ctx);
    g_model = Loaded{};
}

whisper_full_params decode_params(int beam_size, int threads) {
    whisper_full_params wp = whisper_full_default_params(WHISPER_SAMPLING_BEAM_SEARCH);
    wp.beam_search.beam_size = beam_size;
    wp.greedy.best_of = 1;
    wp.n_threads = threads;
    wp.language = "en";
    wp.translate = false;
    wp.no_context = true;         // no previous-text conditioning
    wp.temperature = 0.0f;
    wp.temperature_inc = 0.0f;    // no temperature fallback
    wp.suppress_nst = true;       // parity with faster-whisper suppress_tokens=[-1]
    wp.print_progress = false;
    wp.print_realtime = false;
    wp.print_timestamps = false;
    wp.print_special = false;
    wp.abort_callback = [](void *) { return g_cancel.load(); };
    return wp;
}

// Which backend did whisper really initialize? whisper silently falls back to
// CPU when the GPU backend fails, so we read its own allocation report: the
// state's "using VulkanN backend" line and the model weights' buffer name.
std::string detect_backend(const std::string &log, int ggml_index) {
    if (ggml_index < 0) return "cpu";
    const std::string dev = "Vulkan" + std::to_string(ggml_index);
    const bool backend_up = log.find("using " + dev + " backend") != std::string::npos &&
                            log.find("failed to initialize " + dev) == std::string::npos;
    const std::regex weights("whisper_model_load:\\s+" + dev + " total size");
    const bool weights_on_gpu = std::regex_search(log, weights);
    return backend_up && weights_on_gpu ? "vulkan" : "cpu";
}

json handle_load(const json &req) {
    const json id = req["id"];
    ensure_devices();
#ifdef VOICEWAVE_ASR_VULKAN
    // Without a working loader, ggml's registry init could fault on the missing
    // delay-loaded import. Refuse everything; the app uses the CPU worker.
    if (!g_devices.vulkan_ok) return fail(id, "backend", "Vulkan unavailable: " + g_devices.vulkan_error);
#endif
    if (!req.contains("modelPath") || !req["modelPath"].is_string()) return fail(id, "protocol", "modelPath is required");
    const std::string device = req.value("device", "cpu");
    if (device != "cpu" && device != "gpu") return fail(id, "protocol", "device must be \"gpu\" or \"cpu\"");
    const bool want_gpu = device == "gpu";
    const int threads = req.value("threads", default_threads());
    const bool flash_attn = req.value("flashAttention", false);
    const bool warmup = req.value("warmup", true);

    const Device *target = nullptr;
    if (want_gpu) {
        const std::string luid = req.contains("luid") && req["luid"].is_string() ? req["luid"].get<std::string>() : "";
        const int index = req.contains("ggmlIndex") && req["ggmlIndex"].is_number_integer() ? req["ggmlIndex"].get<int>() : -1;
        if (luid.empty() && index < 0) return fail(id, "protocol", "device \"gpu\" needs luid or ggmlIndex");
        for (auto &d : g_devices.devices) {
            if (d.ggml_index < 0) continue;
            if (!luid.empty() && d.luid != luid) continue;
            if (index >= 0 && d.ggml_index != index) continue;
            target = &d;
            break;
        }
        if (!target) return fail(id, "backend", "requested GPU is not present or not usable");
    }

    unload();

    // Cross-check the by-construction mapping against what ggml reports.
    if (target) {
        int k = 0;
        ggml_backend_dev_t gdev = nullptr;
        for (size_t i = 0; i < ggml_backend_dev_count(); i++) {
            ggml_backend_dev_t d = ggml_backend_dev_get(i);
            const auto type = ggml_backend_dev_type(d);
            if (type != GGML_BACKEND_DEVICE_TYPE_GPU && type != GGML_BACKEND_DEVICE_TYPE_IGPU) continue;
            if (k++ == target->ggml_index) gdev = d;
        }
        if (!gdev) return fail(id, "backend", "ggml does not list the requested GPU");
        ggml_backend_dev_props props{};
        ggml_backend_dev_get_props(gdev, &props);
        const bool name_ok = props.description && target->name == props.description;
        const bool pci_ok = !props.device_id || target->pci_bus_id.empty() || target->pci_bus_id == props.device_id;
        if (!name_ok || !pci_ok) {
            return fail(id, "backend", std::string("ggml device mapping mismatch: expected ") + target->name +
                                           ", ggml reports " + (props.description ? props.description : "?"));
        }
    }

    const std::string path = req["modelPath"].get<std::string>();
    FILE *f = _wfopen(utf8_to_wide(path).c_str(), L"rb");
    if (!f) return fail(id, "artifact", "cannot open model file: " + path);
    whisper_model_loader loader{f, file_read, file_eof, file_close};

    whisper_context_params cp = whisper_context_default_params();
    cp.use_gpu = want_gpu;
    cp.gpu_device = target ? target->ggml_index : 0;
    cp.flash_attn = flash_attn;

    LogCapture capture;
    const double t0 = now_ms();
    whisper_context *ctx = nullptr;
    try {
        ctx = whisper_init_with_params(&loader, cp);  // closes the file via loader.close
    } catch (const std::bad_alloc &) {
        return fail(id, "resource", "out of memory while loading the model");
    } catch (const std::exception &e) {
        return fail(id, classify(e.what(), want_gpu ? "backend" : "artifact"), std::string("model load failed: ") + e.what());
    }
    const double load_ms = now_ms() - t0;
    const std::string log = capture.snapshot();
    if (!ctx) {
        const std::string detail = last_error_line(log);
        return fail(id, classify(log, want_gpu ? "backend" : "artifact"),
                    "model load failed" + (detail.empty() ? std::string() : ": " + detail));
    }

    const std::string backend = detect_backend(log, target ? target->ggml_index : -1);
    if (want_gpu && backend != "vulkan") {
        whisper_free(ctx);
        return fail(id, classify(log, "backend"), "GPU backend did not initialize; whisper fell back to CPU");
    }

    g_model.ctx = ctx;
    g_model.backend = backend;
    g_model.threads = threads;
    if (target) {
        g_model.device_name = target->name;
        g_model.luid = target->luid;
        g_model.ggml_index = target->ggml_index;
    } else {
        g_model.device_name = "CPU";
    }

    // Warm-up: the first decode after load pays one-time GPU pipeline setup.
    double warmup_ms = 0;
    if (warmup) {
        std::vector<float> silence(kSampleRate, 0.0f);
        whisper_full_params wp = decode_params(5, threads);
        wp.abort_callback = nullptr;
        const double tw = now_ms();
        if (whisper_full(ctx, wp, silence.data(), (int) silence.size()) != 0) {
            const std::string wlog = capture.snapshot();
            unload();
            return fail(id, classify(wlog, "backend"), "warm-up decode failed");
        }
        warmup_ms = now_ms() - tw;
    }

    json out = {{"id", id},
                {"ok", true},
                {"backendUsed", g_model.backend},
                {"deviceName", g_model.device_name},
                {"modelInitMs", (int64_t) load_ms},
                {"warmupMs", (int64_t) warmup_ms},
                {"threads", threads},
                {"flashAttention", flash_attn}};
    if (target) {
        out["ggmlIndex"] = target->ggml_index;
        out["luid"] = target->luid.empty() ? json(nullptr) : json(target->luid);
    }
    return out;
}

json handle_transcribe(const json &req) {
    const json id = req["id"];
    if (!g_model.ctx) return fail(id, "protocol", "no model loaded");
    if (req.value("sampleRateHz", kSampleRate) != kSampleRate) return fail(id, "protocol", "sampleRateHz must be 16000");
    if (!req.contains("audioPcm16B64") || !req["audioPcm16B64"].is_string()) return fail(id, "protocol", "audioPcm16B64 is required");
    const std::string language = req.value("language", "en");
    if (language != "en") return fail(id, "protocol", "only language \"en\" is supported");
    const int beam_size = req.value("beamSize", 5);
    if (beam_size < 1 || beam_size > 8) return fail(id, "protocol", "beamSize must be 1..8");

    std::vector<uint8_t> bytes;
    if (!base64_decode(req["audioPcm16B64"].get_ref<const std::string &>(), bytes) || bytes.size() % 2 != 0) {
        return fail(id, "protocol", "invalid audioPcm16B64 payload");
    }
    const size_t n = bytes.size() / 2;
    std::vector<float> pcm(n);
    const int16_t *samples = reinterpret_cast<const int16_t *>(bytes.data());
    for (size_t i = 0; i < n; i++) pcm[i] = samples[i] / 32768.0f;
    const int64_t audio_ms = (int64_t) (n * 1000 / kSampleRate);

    json out = {{"id", id}, {"backendUsed", g_model.backend}, {"deviceName", g_model.device_name}, {"audioMs", audio_ms}};
    if (n == 0) {
        out.update({{"ok", true}, {"text", ""}, {"segmentCount", 0}, {"decodeComputeMs", 0}});
        return out;
    }

    whisper_full_params wp = decode_params(beam_size, g_model.threads);
    std::string prompt;
    if (req.contains("initialPrompt") && req["initialPrompt"].is_string()) {
        prompt = req["initialPrompt"].get<std::string>();
        if (!prompt.empty()) wp.initial_prompt = prompt.c_str();
    }

    {
        std::lock_guard<std::mutex> l(g_running_mu);
        g_running_id = id;
        g_cancel = false;
    }
    LogCapture capture;
    const double t0 = now_ms();
    int rc = 0;
    std::string exception;
    try {
        rc = whisper_full(g_model.ctx, wp, pcm.data(), (int) pcm.size());
    } catch (const std::exception &e) {
        rc = -1;
        exception = e.what();
    }
    const double decode_ms = now_ms() - t0;
    bool cancelled;
    {
        std::lock_guard<std::mutex> l(g_running_mu);
        g_running_id = nullptr;
        cancelled = g_cancel.exchange(false);
    }
    if (cancelled) {
        out.update({{"ok", false}, {"cancelled", true}, {"error", "cancelled"}});
        return out;
    }
    if (rc != 0) {
        const std::string log = capture.snapshot() + exception;
        const std::string detail = exception.empty() ? last_error_line(log) : exception;
        return fail(id, classify(log, "backend"), "decode failed" + (detail.empty() ? std::string() : ": " + detail));
    }

    std::string text;
    double logprob_sum = 0, no_speech_sum = 0;
    int logprob_n = 0;
    const int segments = whisper_full_n_segments(g_model.ctx);
    const whisper_token eot = whisper_token_eot(g_model.ctx);
    for (int s = 0; s < segments; s++) {
        std::string seg = whisper_full_get_segment_text(g_model.ctx, s);
        const size_t a = seg.find_first_not_of(" \t\n");
        const size_t b = seg.find_last_not_of(" \t\n");
        if (a != std::string::npos) {
            if (!text.empty()) text += ' ';
            text += seg.substr(a, b - a + 1);
        }
        no_speech_sum += whisper_full_get_segment_no_speech_prob(g_model.ctx, s);
        for (int t = 0; t < whisper_full_n_tokens(g_model.ctx, s); t++) {
            const whisper_token_data td = whisper_full_get_token_data(g_model.ctx, s, t);
            if (td.id >= eot) continue;  // special and timestamp tokens
            logprob_sum += td.plog;
            logprob_n++;
        }
    }
    out.update({{"ok", true},
                {"text", text},
                {"segmentCount", segments},
                {"decodeComputeMs", (int64_t) decode_ms},
                {"avgLogProb", logprob_n ? logprob_sum / logprob_n : 0.0},
                {"noSpeechProb", segments ? no_speech_sum / segments : 0.0}});
    return out;
}

// --------------------------------------------------------------- main loop ---

std::mutex g_queue_mu;
std::condition_variable g_queue_cv;
std::deque<json> g_queue;

// stdin reader: cancel and shutdown act immediately, everything else is queued
// for the main thread in arrival order. stdin EOF means the app is gone.
void reader_thread() {
    std::string line;
    while (std::getline(std::cin, line)) {
        if (!line.empty() && line.back() == '\r') line.pop_back();
        if (line.find_first_not_of(" \t") == std::string::npos) continue;
        json req = json::parse(line, nullptr, false);
        if (req.is_discarded() || !req.is_object()) {
            emit(fail(nullptr, "protocol", "request is not a JSON object"));
            continue;
        }
        const json id = req.contains("id") ? req["id"] : json(nullptr);
        if (!id.is_string() && !id.is_number_integer()) {
            emit(fail(nullptr, "protocol", "request id (string or integer) is required"));
            continue;
        }
        const std::string command = req.contains("command") && req["command"].is_string() ? req["command"].get<std::string>() : "";
        if (command == "cancel") {
            bool hit = false;
            {
                std::lock_guard<std::mutex> l(g_running_mu);
                if (!g_running_id.is_null() && (!req.contains("targetId") || req["targetId"] == g_running_id)) {
                    g_cancel = true;
                    hit = true;
                }
            }
            emit({{"id", id}, {"ok", true}, {"cancelled", hit}});
        } else if (command == "shutdown") {
            emit({{"id", id}, {"ok", true}, {"shutdown", true}});
            exit_now(0);
        } else if (command == "list_devices" || command == "load" || command == "transcribe") {
            std::lock_guard<std::mutex> l(g_queue_mu);
            g_queue.push_back(std::move(req));
            g_queue_cv.notify_one();
        } else {
            emit(fail(id, "protocol", "unknown command: " + command));
        }
    }
    exit_now(0);
}

}  // namespace

int main(int argc, char **argv) {
    // First: never show a Windows error dialog, even on a crash or GGML_ABORT.
    SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
    _set_abort_behavior(0, _WRITE_ABORT_MSG | _CALL_REPORTFAULT);

    // Keep the real stdout for JSON only; anything else written to fd 1 or
    // STD_OUTPUT_HANDLE (stray printf in a library) lands on stderr.
    _setmode(_fileno(stdin), _O_BINARY);
    const int json_fd = _dup(_fileno(stdout));
    g_out = _fdopen(json_fd, "wb");
    _dup2(_fileno(stderr), _fileno(stdout));
    SetStdHandle(STD_OUTPUT_HANDLE, GetStdHandle(STD_ERROR_HANDLE));
    if (!g_out) return 2;

    whisper_log_set(log_callback, nullptr);
    ggml_log_set(log_callback, nullptr);

    if (argc > 1 && strcmp(argv[1], "--list-devices") == 0) {
        emit(device_list_response(nullptr));
        exit_now(0);
    }

    emit({{"ready", true},
          {"protocol", kProtocolVersion},
          {"variant", VOICEWAVE_ASR_VARIANT},
          {"whisperVersion", whisper_version()}});

    std::thread(reader_thread).detach();
    for (;;) {
        json req;
        {
            std::unique_lock<std::mutex> l(g_queue_mu);
            g_queue_cv.wait(l, [] { return !g_queue.empty(); });
            req = std::move(g_queue.front());
            g_queue.pop_front();
        }
        const std::string command = req["command"].get<std::string>();
        json res;
        try {
            if (command == "list_devices") res = device_list_response(req["id"]);
            else if (command == "load") res = handle_load(req);
            else res = handle_transcribe(req);
        } catch (const json::exception &e) {
            res = fail(req["id"], "protocol", std::string("bad request field: ") + e.what());
        } catch (const std::bad_alloc &) {
            res = fail(req["id"], "resource", "out of memory");
        } catch (const std::exception &e) {
            res = fail(req["id"], classify(e.what(), "backend"), e.what());
        }
        emit(res);
    }
}
