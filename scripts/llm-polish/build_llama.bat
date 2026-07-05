@echo off
REM Build the SHIPPING CPU-only llama-cpp-python wheel: AVX2/FMA/F16C yes
REM (broad compatibility -- the stock PyPI wheel uses AVX-512 and crashes with
REM 0xc000001d on CPUs that lack it), AVX-512 no, CUDA OFF.
REM
REM Produces the wheel in scripts\llm-polish\wheelhouse (the ONLY wheel that
REM may live there -- build-embedded-runtime.ps1 bakes it into the installer
REM runtime, so it must load on machines with no NVIDIA GPU / CUDA runtime).
REM GPU experiments belong in build_llama_cuda.bat, which writes elsewhere.
REM Afterwards it installs the built wheel into the dev venv for local use.
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
set "PATH=%PATH%;C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin"
set "PATH=%PATH%;C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja"
set "FORCE_CMAKE=1"
set "CMAKE_GENERATOR=Ninja"
set "CMAKE_ARGS=-DGGML_NATIVE=OFF -DGGML_AVX=ON -DGGML_AVX2=ON -DGGML_FMA=ON -DGGML_F16C=ON -DGGML_AVX512=OFF -DGGML_CUDA=OFF"
"C:\Users\posan\OneDrive\Desktop\voice vibe\.venv-faster-whisper\Scripts\python.exe" -m pip wheel "llama-cpp-python==0.3.32" --no-binary llama-cpp-python --no-deps -w "C:\Users\posan\OneDrive\Desktop\voice vibe\scripts\llm-polish\wheelhouse" --no-cache-dir
if errorlevel 1 exit /b 1
"C:\Users\posan\OneDrive\Desktop\voice vibe\.venv-faster-whisper\Scripts\python.exe" -m pip install --force-reinstall --no-deps "llama-cpp-python==0.3.32" --no-index --find-links "C:\Users\posan\OneDrive\Desktop\voice vibe\scripts\llm-polish\wheelhouse"
