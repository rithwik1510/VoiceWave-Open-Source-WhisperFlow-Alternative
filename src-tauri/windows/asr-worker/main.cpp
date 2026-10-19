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
//   on start   -> {"ready":true,"protocol":1,"variant":"vk"|"cpu","whisperVersion":"1.9.5",
//                  "engines":["whisper","parakeet"]}
//   {"id":..,"command":"list_devices"}
//   {"id":..,"command":"load","modelPath":..,"engine"?:"whisper"|"parakeet","device":"gpu"|"cpu",
//                             "luid"?:..,"ggmlIndex"?:n,"flashAttention"?:bool,"threads"?:n,"warmup"?:bool}
//   {"id":..,"command":"transcribe","audioPcm16B64":..,"sampleRateHz"?:16000,"beamSize"?:5,
//                                   "language"?:"en","initialPrompt"?:..,
//                                   "boostTerms"?:[..],"boostStrength"?:1.0}
//   {"id":..,"command":"cancel","targetId"?:..}   abandons the running transcribe
//   {"id":..,"command":"shutdown"}
//
// Engines: "whisper" (default) runs whisper.cpp; "parakeet" runs NVIDIA Parakeet
// TDT via whisper.cpp's parakeet.h. Parakeet decodes greedily, detects the
// language itself and has no prompt, so it ignores beamSize, language,
// initialPrompt and flashAttention; its noSpeechProb is always 0. Instead of a
// prompt it takes dictionary terms as boostTerms (plan 021, see "boosting"
// below); its transcribe reply then adds
// "boost":{"terms","skipped","completed","flipped","ms"}. Whisper ignores them.
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
#include <cmath>
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
#include <unordered_set>
#include <vector>

#include "ggml-backend.h"
#include "json.hpp"
#include "parakeet.h"
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

std::string wide_to_utf8(const std::wstring &w) {
    if (w.empty()) return {};
    int n = WideCharToMultiByte(CP_UTF8, 0, w.data(), (int) w.size(), nullptr, 0, nullptr, nullptr);
    if (n <= 0) return {};
    std::string s((size_t) n, '\0');
    WideCharToMultiByte(CP_UTF8, 0, w.data(), (int) w.size(), s.data(), n, nullptr, nullptr);
    return s;
}

// Unicode case mapping (LCMAP_LOWERCASE / LCMAP_UPPERCASE), ASCII fast path.
// Invalid UTF-8 comes back unchanged.
std::string utf8_case(const std::string &s, DWORD flag) {
    if (std::all_of(s.begin(), s.end(), [](unsigned char c) { return c < 0x80; })) {
        std::string out(s);
        for (char &c : out) c = (char) (flag == LCMAP_LOWERCASE ? tolower((unsigned char) c) : toupper((unsigned char) c));
        return out;
    }
    const std::wstring w = utf8_to_wide(s);
    if (w.empty()) return s;
    std::wstring mapped(w.size(), L'\0');
    if (LCMapStringEx(LOCALE_NAME_INVARIANT, flag, w.data(), (int) w.size(), mapped.data(), (int) mapped.size(),
                      nullptr, nullptr, 0) != (int) w.size()) {
        return s;
    }
    return wide_to_utf8(mapped);
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
    whisper_context *ctx = nullptr;    // engine "whisper"
    parakeet_context *pctx = nullptr;  // engine "parakeet"; at most one of the two is set
    std::string backend;  // "vulkan" | "cpu": what the engine actually initialized
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
    if (g_model.pctx) parakeet_free(g_model.pctx);
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

parakeet_full_params parakeet_decode_params(int threads) {
    parakeet_full_params pp = parakeet_full_default_params(PARAKEET_SAMPLING_GREEDY);
    pp.n_threads = threads;
    pp.no_context = true;
    pp.abort_callback = [](void *) { return g_cancel.load(); };
    return pp;
}

// -------------------------------------------------------------- boosting ---
// Dictionary boosting for Parakeet (plan 021): shallow fusion with a term
// trie inside the greedy decoder. Before each token choice, word-pieces that
// continue spelling a dictionary term get a bonus on their raw logit (so the
// term wins ambiguous audio and loses clear audio). Matching is
// case-insensitive on the piece text, so any tokenization of a term matches.
// A completed term is re-cased to the dictionary spelling only when the bonus
// changed at least one decision inside it.

// Raw-logit bonuses, tuned on the plan 021 test set (TTS clean and 10 dB
// noise, 34- and 200-term dictionaries): the highest that changed no trap
// sentence. A start bonus beyond ~2 lets a term's first piece displace common
// words under noise; a continuation bonus beyond ~8 spells "cloud" as "claud".
constexpr float kBoostStart = 1.5f;  // bonus for a piece that starts a term
constexpr float kBoostCont = 7.0f;   // bonus for a piece that continues one
constexpr int kBoostTopK = 16;       // candidates considered per step
constexpr size_t kMaxBoostTerms = 200;
constexpr size_t kMaxActiveMatches = 16;
const std::string kSpmSpace = "\xE2\x96\x81";  // SentencePiece word marker

// Per-model piece tables, built once at load.
struct PieceTable {
    std::vector<std::string> raw;     // piece as in the vocab ("▁Claude")
    std::vector<std::string> lower;   // lowercased, for matching
    std::vector<char> word_start;     // starts with ▁
    std::vector<char> starts_match;   // ▁ plus a character: may open a match when emitted
    std::vector<char> start_bonus;    // ▁ plus at least 2 characters: may get the start bonus
    std::unordered_set<std::string> set;  // raw pieces, for the known-word check
};
PieceTable g_pieces;

void build_piece_table(parakeet_context *pctx) {
    g_pieces = PieceTable{};
    const int n = parakeet_n_vocab(pctx);
    for (int i = 0; i < n; i++) {
        const std::string piece = parakeet_token_to_str(pctx, i);
        g_pieces.lower.push_back(utf8_case(piece, LCMAP_LOWERCASE));
        const bool word_start = piece.rfind(kSpmSpace, 0) == 0;
        // Code points after the ▁. A 1-letter start bonus ("▁T") would tilt
        // every word with that initial and re-case or re-split it ("8GB" ->
        // "8 GB"); but names often begin that way ("▁V ul kan"), so a 1-letter
        // piece the model chose itself still opens a match.
        const size_t chars = word_start ? std::count_if(piece.begin() + kSpmSpace.size(), piece.end(),
                                                        [](unsigned char c) { return (c & 0xC0) != 0x80; })
                                        : 0;
        g_pieces.word_start.push_back(word_start);
        g_pieces.starts_match.push_back(chars >= 1);
        g_pieces.start_bonus.push_back(chars >= 2);
        g_pieces.set.insert(piece);
        g_pieces.raw.push_back(piece);
    }
}

struct TermTrie {
    struct Node {
        std::vector<std::pair<char, int>> next;
        int term = -1;  // index into Booster::terms when a term ends here
    };
    std::vector<Node> nodes{1};  // node 0 is the root

    int walk(int node, const std::string &s) const {
        for (char c : s) {
            const auto &next = nodes[node].next;
            auto it = std::find_if(next.begin(), next.end(), [c](const auto &e) { return e.first == c; });
            if (it == next.end()) return -1;
            node = it->second;
        }
        return node;
    }

    void insert(const std::string &key, int term) {
        int node = 0;
        for (char c : key) {
            int child = walk(node, std::string(1, c));
            if (child < 0) {
                child = (int) nodes.size();
                nodes[node].next.push_back({c, child});
                nodes.emplace_back();
            }
            node = child;
        }
        nodes[node].term = term;
    }
};

struct Booster {
    float start = kBoostStart, cont = kBoostCont;
    TermTrie trie;
    std::vector<std::string> terms;  // dictionary spelling, by trie term index
    int skipped = 0;

    struct Match {
        int node;
        int first;     // index of its first piece in `emitted`
        bool flipped;  // the bonus changed a decision inside it
    };
    struct Span {
        int first, last, term;
    };
    std::vector<Match> active, scratch;
    std::vector<int> emitted;  // token ids, in order
    std::vector<Span> recase;
    std::vector<int> cand;
    std::vector<char> kinds;  // Kind per candidate
    int greedy_choice = -1;  // argmax before the bonus, for the step in flight
    int completed = 0, flipped = 0;
    double ms = 0;

    bool enabled() const { return !terms.empty() && (start > 0 || cont > 0); }

    // Normalizes and filters the requested terms into the trie. Skipped:
    // under 3 characters, acronyms (no lowercase letter), and single words
    // the model already has as one vocab piece ("these"; its multilingual
    // vocab holds few whole English words, so most terms pass).
    void add_terms(const json &list) {
        std::unordered_set<std::string> seen;
        for (const auto &item : list) {
            if (!item.is_string()) continue;
            std::string display;
            for (char c : item.get<std::string>()) {
                const bool space = c == ' ' || c == '\t' || c == '\n' || c == '\r';
                if (space) {
                    if (!display.empty() && display.back() != ' ') display += ' ';
                } else {
                    display += c;
                }
            }
            if (!display.empty() && display.back() == ' ') display.pop_back();
            if (display.empty()) continue;
            const std::wstring wide = utf8_to_wide(display);
            const bool has_lower = std::any_of(wide.begin(), wide.end(), [](wchar_t c) { return IsCharLowerW(c); });
            const std::string lower = utf8_case(display, LCMAP_LOWERCASE);
            if (wide.size() < 3 || !has_lower) {
                skipped++;
                continue;
            }
            if (display.find(' ') == std::string::npos) {
                const std::wstring wl = utf8_to_wide(lower);
                const std::string capitalized = utf8_case(wide_to_utf8(wl.substr(0, 1)), LCMAP_UPPERCASE) + wide_to_utf8(wl.substr(1));
                if (g_pieces.set.count(kSpmSpace + lower) || g_pieces.set.count(kSpmSpace + capitalized)) {
                    skipped++;
                    continue;
                }
            }
            std::string key = kSpmSpace;
            for (char c : lower) key += c == ' ' ? kSpmSpace : std::string(1, c);
            if (!seen.insert(key).second) continue;
            if (terms.size() >= kMaxBoostTerms) {
                skipped++;
                continue;
            }
            trie.insert(key, (int) terms.size());
            terms.push_back(display);
        }
    }

    // logits: n token logits, blank last. The blank-vs-token decision is the
    // model's alone: the bonus only reorders word-pieces when the model has
    // already chosen to emit one, so boosting never inserts a word.
    void filter(float *logits, int n) {
        const double t0 = now_ms();
        const int blank = n - 1;
        const int best = (int) (std::max_element(logits, logits + n) - logits);
        greedy_choice = best;
        if (best != blank) {
            const float floor = logits[best] - std::max(start, active.empty() ? 0.0f : cont);
            cand.clear();
            for (int i = 0; i < blank; i++) {
                if (logits[i] > floor) cand.push_back(i);
            }
            if ((int) cand.size() > kBoostTopK) {
                std::partial_sort(cand.begin(), cand.begin() + kBoostTopK, cand.end(),
                                  [logits](int a, int b) { return logits[a] > logits[b]; });
                cand.resize(kBoostTopK);
            }
            // A continuation outranks starting a new term: when one is on
            // offer, start bonuses sit this step out ("▁goat|co" over "▁Co").
            kinds.resize(cand.size());
            bool continuing = false;
            for (size_t k = 0; k < cand.size(); k++) {
                kinds[k] = kind(cand[k], best);
                continuing |= kinds[k] == kCont;
            }
            for (size_t k = 0; k < cand.size(); k++) {
                if (kinds[k] == kCont) logits[cand[k]] += cont;
                if (kinds[k] == kStart && !continuing) logits[cand[k]] += start;
            }
        }
        ms += now_ms() - t0;
    }

    enum Kind : char { kNone, kStart, kCont };

    // One piece is a prefix of the other ignoring case, but not with it.
    static bool case_conflict(int a, int b) {
        const std::string &la = g_pieces.lower[a], &lb = g_pieces.lower[b];
        if (la.size() > lb.size()) return case_conflict(b, a);
        return lb.rfind(la, 0) == 0 && g_pieces.raw[b].rfind(g_pieces.raw[a], 0) != 0;
    }

    // Which bonus `token` earns when the model's own choice is `top`.
    Kind kind(int token, int top) const {
        if (token >= (int) g_pieces.lower.size()) return kNone;
        const std::string &piece = g_pieces.lower[token];
        // Same letters as the model's choice in another case ("▁T" over
        // "▁tem", "▁cl" over "▁C"): winning would only re-case the word.
        if (token != top && case_conflict(token, top)) return kNone;
        for (const Match &m : active) {
            if (trie.walk(m.node, piece) >= 0) return kCont;
        }
        if (g_pieces.start_bonus[token] && trie.walk(0, piece) >= 0) return kStart;
        return kNone;
    }

    void on_token(int token) {
        const double t0 = now_ms();
        const bool flip = token != greedy_choice;
        const int idx = (int) emitted.size();
        emitted.push_back(token);
        if (token < (int) g_pieces.lower.size()) {
            const std::string &piece = g_pieces.lower[token];
            const bool word_start = g_pieces.word_start[token];
            // A term completes at a word boundary: a new word or punctuation.
            if (word_start || ((unsigned char) piece[0] < 0x80 && !isalnum((unsigned char) piece[0]))) {
                for (const Match &m : active) {
                    if (trie.nodes[m.node].term >= 0) complete(m, idx - 1);
                }
            }
            scratch.clear();
            auto keep = [this](int node, int first, bool flipped) {
                if (node < 0 || scratch.size() >= kMaxActiveMatches) return;
                for (Match &m : scratch) {
                    if (m.node == node) {  // same prefix, so the same start
                        m.flipped |= flipped;
                        return;
                    }
                }
                scratch.push_back({node, first, flipped});
            };
            for (const Match &m : active) keep(trie.walk(m.node, piece), m.first, m.flipped || flip);
            if (g_pieces.starts_match[token]) keep(trie.walk(0, piece), idx, flip);
            active.swap(scratch);
        } else {
            active.clear();
        }
        ms += now_ms() - t0;
    }

    void complete(const Match &m, int last) {
        completed++;
        if (!m.flipped) return;
        flipped++;
        recase.push_back({m.first, last, trie.nodes[m.node].term});
    }

    // End of the decode: matches still open at the end of the text complete.
    void finish() {
        for (const Match &m : active) {
            if (trie.nodes[m.node].term >= 0) complete(m, (int) emitted.size() - 1);
        }
        active.clear();
    }

    // The transcript rebuilt from the emitted pieces (as parakeet.cpp builds
    // a segment), with each flipped term in its dictionary spelling. Overlaps
    // keep the earlier, then longer, span ("Claude Code" over "Claude").
    std::string text() {
        std::sort(recase.begin(), recase.end(), [](const Span &a, const Span &b) {
            return a.first != b.first ? a.first < b.first : a.last > b.last;
        });
        std::string out;
        size_t next_span = 0;
        char buf[256];
        for (int i = 0; i < (int) emitted.size();) {
            while (next_span < recase.size() && recase[next_span].first < i) next_span++;
            if (next_span < recase.size() && recase[next_span].first == i) {
                const Span &s = recase[next_span];
                if (!out.empty() && g_pieces.word_start[emitted[i]]) out += ' ';
                out += terms[s.term];
                i = s.last + 1;
                continue;
            }
            const int len = parakeet_token_to_text(g_pieces.raw[emitted[i]].c_str(), out.empty(), buf, sizeof buf);
            out.append(buf, std::min(len, (int) sizeof buf - 1));
            i++;
        }
        return out;
    }
};

// Bonus sizes for this request: the defaults scaled by `boostStrength`. The
// dev env var VOICEWAVE_PARAKEET_BOOST overrides it: "0" disables, "1.5"
// scales, "1.5,3" sets the start and continuation bonuses directly.
void boost_strength(const json &req, Booster &b) {
    float strength = req.value("boostStrength", 1.0f);
    if (const char *env = getenv("VOICEWAVE_PARAKEET_BOOST"); env && *env) {
        float s = 0, c = 0;
        if (sscanf(env, "%f,%f", &s, &c) == 2) {
            b.start = s;
            b.cont = c;
            return;
        }
        strength = (float) atof(env);
    }
    strength = std::clamp(strength, 0.0f, 4.0f);
    b.start = kBoostStart * strength;
    b.cont = kBoostCont * strength;
}

// Which backend did the engine really initialize? whisper and parakeet both
// silently fall back to CPU when the GPU backend fails, so we read their own
// allocation report: the state's "using VulkanN backend" line and the model
// weights' buffer name, logged by `model_load_fn`.
std::string detect_backend(const std::string &log, int ggml_index, const char *model_load_fn) {
    if (ggml_index < 0) return "cpu";
    const std::string dev = "Vulkan" + std::to_string(ggml_index);
    const bool backend_up = log.find("using " + dev + " backend") != std::string::npos &&
                            log.find("failed to initialize " + dev) == std::string::npos;
    const std::regex weights(std::string(model_load_fn) + ":\\s+" + dev + " total size");
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
    const std::string engine = req.value("engine", "whisper");
    if (engine != "whisper" && engine != "parakeet") return fail(id, "protocol", "engine must be \"whisper\" or \"parakeet\"");
    const bool parakeet = engine == "parakeet";
    const std::string device = req.value("device", "cpu");
    if (device != "cpu" && device != "gpu") return fail(id, "protocol", "device must be \"gpu\" or \"cpu\"");
    const bool want_gpu = device == "gpu";
    const int threads = req.value("threads", default_threads());
    const bool flash_attn = !parakeet && req.value("flashAttention", false);
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

    LogCapture capture;
    const double t0 = now_ms();
    whisper_context *ctx = nullptr;
    parakeet_context *pctx = nullptr;
    try {
        // Both loaders close the file via loader.close.
        if (parakeet) {
            parakeet_model_loader loader{f, file_read, file_eof, file_close};
            parakeet_context_params pcp = parakeet_context_default_params();
            pcp.use_gpu = want_gpu;
            pcp.gpu_device = target ? target->ggml_index : 0;
            pctx = parakeet_init_with_params(&loader, pcp);
        } else {
            whisper_model_loader loader{f, file_read, file_eof, file_close};
            whisper_context_params cp = whisper_context_default_params();
            cp.use_gpu = want_gpu;
            cp.gpu_device = target ? target->ggml_index : 0;
            cp.flash_attn = flash_attn;
            ctx = whisper_init_with_params(&loader, cp);
        }
    } catch (const std::bad_alloc &) {
        return fail(id, "resource", "out of memory while loading the model");
    } catch (const std::exception &e) {
        return fail(id, classify(e.what(), want_gpu ? "backend" : "artifact"), std::string("model load failed: ") + e.what());
    }
    const double load_ms = now_ms() - t0;
    const std::string log = capture.snapshot();
    if (!ctx && !pctx) {
        const std::string detail = last_error_line(log);
        return fail(id, classify(log, want_gpu ? "backend" : "artifact"),
                    "model load failed" + (detail.empty() ? std::string() : ": " + detail));
    }

    const std::string backend = detect_backend(log, target ? target->ggml_index : -1,
                                               parakeet ? "parakeet_model_load" : "whisper_model_load");
    if (want_gpu && backend != "vulkan") {
        if (ctx) whisper_free(ctx);
        if (pctx) parakeet_free(pctx);
        return fail(id, classify(log, "backend"),
                    std::string("GPU backend did not initialize; ") + engine + " fell back to CPU");
    }

    g_model.ctx = ctx;
    g_model.pctx = pctx;
    if (pctx) build_piece_table(pctx);
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
        const double tw = now_ms();
        int rc;
        if (parakeet) {
            parakeet_full_params pp = parakeet_decode_params(threads);
            pp.abort_callback = nullptr;
            rc = parakeet_full(pctx, pp, silence.data(), (int) silence.size());
        } else {
            whisper_full_params wp = decode_params(5, threads);
            wp.abort_callback = nullptr;
            rc = whisper_full(ctx, wp, silence.data(), (int) silence.size());
        }
        if (rc != 0) {
            const std::string wlog = capture.snapshot();
            unload();
            return fail(id, classify(wlog, "backend"), "warm-up decode failed");
        }
        warmup_ms = now_ms() - tw;
    }

    json out = {{"id", id},
                {"ok", true},
                {"engine", engine},
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
    const bool parakeet = g_model.pctx != nullptr;
    if (!g_model.ctx && !parakeet) return fail(id, "protocol", "no model loaded");
    if (req.value("sampleRateHz", kSampleRate) != kSampleRate) return fail(id, "protocol", "sampleRateHz must be 16000");
    if (!req.contains("audioPcm16B64") || !req["audioPcm16B64"].is_string()) return fail(id, "protocol", "audioPcm16B64 is required");
    const std::string language = req.value("language", "en");
    if (!parakeet && language != "en") return fail(id, "protocol", "only language \"en\" is supported");
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
    if (!parakeet && req.contains("initialPrompt") && req["initialPrompt"].is_string()) {
        prompt = req["initialPrompt"].get<std::string>();
        if (!prompt.empty()) wp.initial_prompt = prompt.c_str();
    }
    Booster booster;
    const bool boost_requested = parakeet && req.contains("boostTerms") && req["boostTerms"].is_array();
    if (boost_requested) {
        boost_strength(req, booster);
        booster.add_terms(req["boostTerms"]);
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
        if (parakeet) {
            // parakeet_full encodes the whole clip in one graph (up to ~10 min
            // before it switches to local attention), so long clips need no
            // chunking here.
            parakeet_full_params pp = parakeet_decode_params(g_model.threads);
            if (booster.enabled()) {
                pp.logits_filter_callback = [](parakeet_context *, parakeet_state *, float *logits, int n, void *b) {
                    static_cast<Booster *>(b)->filter(logits, n);
                };
                pp.logits_filter_callback_user_data = &booster;
                pp.new_token_callback = [](parakeet_context *, parakeet_state *, const parakeet_token_data *td, void *b) {
                    static_cast<Booster *>(b)->on_token(td->id);
                };
                pp.new_token_callback_user_data = &booster;
            }
            rc = parakeet_full(g_model.pctx, pp, pcm.data(), (int) pcm.size());
        } else {
            rc = whisper_full(g_model.ctx, wp, pcm.data(), (int) pcm.size());
        }
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
    auto append_segment = [&text](const std::string &seg) {
        const size_t a = seg.find_first_not_of(" \t\n");
        const size_t b = seg.find_last_not_of(" \t\n");
        if (a != std::string::npos) {
            if (!text.empty()) text += ' ';
            text += seg.substr(a, b - a + 1);
        }
    };
    int segments;
    if (parakeet) {
        booster.finish();
        segments = parakeet_full_n_segments(g_model.pctx);
        for (int s = 0; s < segments; s++) {
            // A single segment per decode, so the boosted rebuild replaces it.
            append_segment(booster.recase.empty() ? parakeet_full_get_segment_text(g_model.pctx, s) : booster.text());
            // Parakeet's own plog is not a log-probability; ln(p) of each
            // emitted token is comparable to whisper's avg_logprob.
            for (int t = 0; t < parakeet_full_n_tokens(g_model.pctx, s); t++) {
                const float p = parakeet_full_get_token_p(g_model.pctx, s, t);
                logprob_sum += std::log(std::max(p, 1e-6f));
                logprob_n++;
            }
        }
    } else {
        segments = whisper_full_n_segments(g_model.ctx);
        const whisper_token eot = whisper_token_eot(g_model.ctx);
        for (int s = 0; s < segments; s++) {
            append_segment(whisper_full_get_segment_text(g_model.ctx, s));
            no_speech_sum += whisper_full_get_segment_no_speech_prob(g_model.ctx, s);
            for (int t = 0; t < whisper_full_n_tokens(g_model.ctx, s); t++) {
                const whisper_token_data td = whisper_full_get_token_data(g_model.ctx, s, t);
                if (td.id >= eot) continue;  // special and timestamp tokens
                logprob_sum += td.plog;
                logprob_n++;
            }
        }
    }
    out.update({{"ok", true},
                {"text", text},
                {"segmentCount", segments},
                {"decodeComputeMs", (int64_t) decode_ms},
                {"avgLogProb", logprob_n ? logprob_sum / logprob_n : 0.0},
                {"noSpeechProb", segments ? no_speech_sum / segments : 0.0}});
    if (boost_requested) {
        // Counts only: no term or transcript text leaves through diagnostics.
        out["boost"] = {{"terms", booster.terms.size()},
                        {"skipped", booster.skipped},
                        {"completed", booster.completed},
                        {"flipped", booster.flipped},
                        {"ms", std::round(booster.ms * 100) / 100}};
    }
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
    parakeet_log_set(log_callback, nullptr);
    ggml_log_set(log_callback, nullptr);

    if (argc > 1 && strcmp(argv[1], "--list-devices") == 0) {
        emit(device_list_response(nullptr));
        exit_now(0);
    }

    emit({{"ready", true},
          {"protocol", kProtocolVersion},
          {"variant", VOICEWAVE_ASR_VARIANT},
          {"whisperVersion", whisper_version()},
          {"engines", {"whisper", "parakeet"}}});

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
