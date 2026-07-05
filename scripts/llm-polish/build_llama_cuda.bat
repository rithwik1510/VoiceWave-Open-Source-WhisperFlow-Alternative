@echo off
REM Build LATEST llama-cpp-python WITH CUDA for RTX 3060 (sm_86). Latest bundles a
REM llama.cpp new enough for CUDA 13.1 (the pinned 0.3.32 predated it -> build failed).
REM
REM OUTPUT GOES TO wheelhouse-cuda (gitignored), NEVER scripts\llm-polish\wheelhouse:
REM the shipping wheelhouse is baked into every user's installer by
REM build-embedded-runtime.ps1, and a CUDA-linked wheel there crashes AI polish
REM on every machine without the CUDA runtime (this exact mistake broke the
REM v0.5.0 release builds -- the CUDA wheel had been committed as if it were
REM the CPU one).
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
set "PATH=%PATH%;C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin"
REM Ninja generator avoids the MSBuild "No CUDA toolset found" error; nvcc must be on PATH.
set "PATH=%PATH%;C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja"
set "PATH=%PATH%;%CUDA_PATH%\bin"
set "FORCE_CMAKE=1"
set "CMAKE_GENERATOR=Ninja"
set "CMAKE_ARGS=-DGGML_CUDA=ON -DGGML_NATIVE=OFF -DGGML_AVX=ON -DGGML_AVX2=ON -DGGML_FMA=ON -DGGML_F16C=ON -DGGML_AVX512=OFF -DCMAKE_CUDA_ARCHITECTURES=86"
"C:\Users\posan\OneDrive\Desktop\voice vibe\.venv-faster-whisper\Scripts\python.exe" -m pip wheel llama-cpp-python --no-binary llama-cpp-python -w "C:\Users\posan\OneDrive\Desktop\voice vibe\scripts\llm-polish\wheelhouse-cuda" --no-cache-dir
