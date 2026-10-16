# protobuf-src 2.1.1+27.1 bundles protobuf with static MSVC CRT enabled and
# Abseil with it disabled. Mixing those defaults breaks protoc's link step.
# Keep both on the DLL CRT used by the native Rust MSVC build. These options
# affect only protobuf/Abseil, not other CMake projects' runtime selection.
# FORCE also repairs a CMake cache left by an earlier failed native build.
if(CMAKE_HOST_WIN32)
    set(protobuf_MSVC_STATIC_RUNTIME OFF CACHE BOOL "Use the same DLL CRT as Abseil" FORCE)
    set(ABSL_MSVC_STATIC_RUNTIME OFF CACHE BOOL "Use the same DLL CRT as protobuf" FORCE)
endif()
