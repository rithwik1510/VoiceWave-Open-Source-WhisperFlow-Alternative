# VoiceWave speech worker (plan 019): whisper.cpp in its own process.
#
# One source, two variants, each configured in its own build tree:
#   -DVOICEWAVE_ASR_VULKAN=ON   -> voicewave-asr-vk.exe  (Vulkan + CPU)
#   -DVOICEWAVE_ASR_VULKAN=OFF  -> voicewave-asr-cpu.exe (CPU only, no Vulkan anywhere)
#
# Build with scripts/asr-worker/build-asr-worker.ps1 (MSVC + Ninja).
cmake_minimum_required(VERSION 3.21)

# Static CRT for our code AND for the fetched whisper/ggml targets. whisper.cpp's
# own CMakeLists declares cmake_minimum_required(3.5), which leaves CMP0091
# unset in its scope; the policy default makes CMAKE_MSVC_RUNTIME_LIBRARY apply
# there too. The dumpbin gate in CI verifies no vcruntime/msvcp import remains.
set(CMAKE_POLICY_DEFAULT_CMP0091 NEW)
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>")

project(voicewave_asr_worker LANGUAGES C CXX)

option(VOICEWAVE_ASR_VULKAN "Build the Vulkan variant (voicewave-asr-vk.exe)" OFF)

if(NOT MSVC)
  message(FATAL_ERROR "The VoiceWave speech worker is built with MSVC only.")
endif()

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# --- whisper.cpp / ggml build options (plan 019, "Build and packaging") ---
set(BUILD_SHARED_LIBS OFF CACHE BOOL "" FORCE)        # static whisper + ggml
set(GGML_BACKEND_DL OFF CACHE BOOL "" FORCE)
set(GGML_OPENMP OFF CACHE BOOL "" FORCE)              # ggml thread pool, no vcomp140.dll
set(GGML_CCACHE OFF CACHE BOOL "" FORCE)
# Fixed CPU baseline instead of the build machine's CPU. On MSVC, /arch:AVX2
# implies FMA and F16C (ggml defines GGML_FMA/GGML_F16C with it). BMI2 and
# AVX-512 stay off so the worker needs nothing beyond AVX2/FMA/F16C, the same
# baseline as the AI-polish wheel and the app's eligibility check.
set(GGML_NATIVE OFF CACHE BOOL "" FORCE)
set(GGML_SSE42 ON CACHE BOOL "" FORCE)
set(GGML_AVX ON CACHE BOOL "" FORCE)
set(GGML_AVX2 ON CACHE BOOL "" FORCE)
set(GGML_FMA ON CACHE BOOL "" FORCE)
set(GGML_F16C ON CACHE BOOL "" FORCE)
set(GGML_BMI2 OFF CACHE BOOL "" FORCE)
set(GGML_AVX512 OFF CACHE BOOL "" FORCE)
set(GGML_AVX_VNNI OFF CACHE BOOL "" FORCE)
set(GGML_VULKAN ${VOICEWAVE_ASR_VULKAN} CACHE BOOL "" FORCE)
set(WHISPER_BUILD_IS_DEV OFF CACHE BOOL "" FORCE)
set(WHISPER_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(WHISPER_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
set(WHISPER_BUILD_SERVER OFF CACHE BOOL "" FORCE)
set(WHISPER_CURL OFF CACHE BOOL "" FORCE)
set(WHISPER_SDL2 OFF CACHE BOOL "" FORCE)

# Pinned upstream whisper.cpp 1.9.5. To build from a local checkout instead,
# pass -DFETCHCONTENT_SOURCE_DIR_WHISPER=<path> (skips the download).
include(FetchContent)
FetchContent_Declare(
  whisper
  URL https://github.com/ggml-org/whisper.cpp/archive/refs/tags/v1.9.5.tar.gz
  URL_HASH SHA256=ff1a9053feb509ff9d7729703355541ae9690073a6b1c40eb692c962e0dc1720
  DOWNLOAD_EXTRACT_TIMESTAMP TRUE
)
FetchContent_MakeAvailable(whisper)

if(VOICEWAVE_ASR_VULKAN)
  set(worker_name voicewave-asr-vk)
else()
  set(worker_name voicewave-asr-cpu)
endif()

add_executable(${worker_name} main.cpp)
# nlohmann/json ships inside the pinned whisper.cpp tree (examples/json.hpp).
target_include_directories(${worker_name} PRIVATE "${whisper_SOURCE_DIR}/examples")
target_link_libraries(${worker_name} PRIVATE whisper ggml psapi)
target_compile_definitions(${worker_name} PRIVATE
  NOMINMAX WIN32_LEAN_AND_MEAN _CRT_SECURE_NO_WARNINGS
  VOICEWAVE_ASR_VARIANT="$<IF:$<BOOL:${VOICEWAVE_ASR_VULKAN}>,vk,cpu>")

if(VOICEWAVE_ASR_VULKAN)
  find_package(Vulkan REQUIRED)
  target_compile_definitions(${worker_name} PRIVATE VOICEWAVE_ASR_VULKAN=1)
  target_link_libraries(${worker_name} PRIVATE Vulkan::Vulkan delayimp)
  # Delay-load the Vulkan loader: a PC without vulkan-1.dll must still start the
  # worker (which then reports "no Vulkan") instead of failing at process load
  # with a Windows "DLL not found" dialog that SetErrorMode cannot suppress.
  target_link_options(${worker_name} PRIVATE "/DELAYLOAD:vulkan-1.dll")
endif()
