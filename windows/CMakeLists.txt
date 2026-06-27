cmake_minimum_required(VERSION 3.14)
set(PROJECT_NAME "desktop_screenstate")
project(${PROJECT_NAME} LANGUAGES CXX)

# ── Verify cargo is available ─────────────────────────────────────────────────
find_program(CARGO_EXE cargo REQUIRED)
if(NOT CARGO_EXE)
  message(FATAL_ERROR
    "cargo not found. Install Rust from https://rustup.rs/ and ensure "
    "cargo is on PATH before building this plugin.")
endif()

# ── Rust build ────────────────────────────────────────────────────────────────
set(RUST_SOURCE_DIR "${CMAKE_CURRENT_SOURCE_DIR}/rust")
set(RUST_TARGET_DIR "${CMAKE_CURRENT_BINARY_DIR}/rust_build")

if(CMAKE_BUILD_TYPE STREQUAL "Debug")
  set(CARGO_PROFILE "debug")
  set(CARGO_PROFILE_FLAG "")
else()
  set(CARGO_PROFILE "release")
  set(CARGO_PROFILE_FLAG "--release")
endif()

# Cargo defaults to the host triple. On Windows-on-Arm machines that is
# aarch64-pc-windows-msvc even when Flutter is building the x64 variant of the
# app (Flutter falls back to windows-x64 unless arm64 is explicitly
# configured), which produces a DLL whose architecture doesn't match the
# runner exe. Pin the Rust target to the architecture CMake is actually
# building so they always match.
if(CMAKE_VS_PLATFORM_NAME)
  set(SCREENSTATE_TARGET_ARCH "${CMAKE_VS_PLATFORM_NAME}")
elseif(CMAKE_GENERATOR_PLATFORM)
  set(SCREENSTATE_TARGET_ARCH "${CMAKE_GENERATOR_PLATFORM}")
elseif(CMAKE_SYSTEM_PROCESSOR MATCHES "ARM64|aarch64")
  set(SCREENSTATE_TARGET_ARCH "ARM64")
elseif(CMAKE_SYSTEM_PROCESSOR MATCHES "AMD64|x86_64")
  set(SCREENSTATE_TARGET_ARCH "x64")
else()
  set(SCREENSTATE_TARGET_ARCH "x64")
endif()

if(SCREENSTATE_TARGET_ARCH STREQUAL "ARM64")
  set(RUST_TARGET_TRIPLE "aarch64-pc-windows-msvc")
else()
  set(RUST_TARGET_TRIPLE "x86_64-pc-windows-msvc")
endif()

set(RUST_DLL "${RUST_TARGET_DIR}/${RUST_TARGET_TRIPLE}/${CARGO_PROFILE}/desktop_screenstate_rs.dll")

add_custom_command(
  OUTPUT "${RUST_DLL}"
  COMMAND "${CARGO_EXE}" build
          ${CARGO_PROFILE_FLAG}
          --target "${RUST_TARGET_TRIPLE}"
          --manifest-path "${RUST_SOURCE_DIR}/Cargo.toml"
          --target-dir "${RUST_TARGET_DIR}"
  DEPENDS
    "${RUST_SOURCE_DIR}/src/lib.rs"
    "${RUST_SOURCE_DIR}/Cargo.toml"
  COMMENT "Building Rust screenstate DLL (${RUST_TARGET_TRIPLE}, ${CARGO_PROFILE})"
  VERBATIM
)

add_custom_target(desktop_screenstate_rs ALL DEPENDS "${RUST_DLL}")

# ── Install the DLL next to the Flutter executable ────────────────────────────
# Flutter's FFI plugin mechanism copies files listed in the bundled_libraries
# variable into the application's output directory.
set(desktop_screenstate_bundled_libraries
  "${RUST_DLL}"
  PARENT_SCOPE
)
