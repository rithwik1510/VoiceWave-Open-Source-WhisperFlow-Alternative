@echo off
REM Build llama-cpp-python from source for AMD Zen 3 (Ryzen 5800H): AVX2/FMA yes, AVX-512 no.
REM The prebuilt CPU wheel crashed with 0xc000001d (illegal instruction) = AVX-512 in the wheel.
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
set "PATH=%PATH%;C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin"
set "FORCE_CMAKE=1"
set "CMAKE_ARGS=-DGGML_NATIVE=OFF -DGGML_AVX=ON -DGGML_AVX2=ON -DGGML_FMA=ON -DGGML_F16C=ON -DGGML_AVX512=OFF -DGGML_CUDA=OFF"
"C:\Users\posan\OneDrive\Desktop\voice vibe\.venv-faster-whisper\Scripts\python.exe" -m pip install --no-binary llama-cpp-python "llama-cpp-python==0.3.32" --force-reinstall --no-cache-dir
