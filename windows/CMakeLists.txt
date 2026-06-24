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

set(RUST_DLL "${RUST_TARGET_DIR}/${CARGO_PROFILE}/desktop_screenstate_rs.dll")

add_custom_command(
  OUTPUT "${RUST_DLL}"
  COMMAND "${CARGO_EXE}" build
          ${CARGO_PROFILE_FLAG}
          --manifest-path "${RUST_SOURCE_DIR}/Cargo.toml"
          --target-dir "${RUST_TARGET_DIR}"
  DEPENDS
    "${RUST_SOURCE_DIR}/src/lib.rs"
    "${RUST_SOURCE_DIR}/Cargo.toml"
  COMMENT "Building Rust screenstate DLL (${CARGO_PROFILE})"
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
