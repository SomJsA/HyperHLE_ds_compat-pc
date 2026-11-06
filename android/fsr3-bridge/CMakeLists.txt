cmake_minimum_required(VERSION 3.23)
project(ds_fsr3_bridge CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

if(NOT DEFINED FSR3_SDK_ROOT)
    message(FATAL_ERROR "FSR3_SDK_ROOT is required")
endif()

option(FSR3_USE_PREBUILT
    "Link the checked-in arm64 FidelityFX static libraries instead of rebuilding the SDK"
    ON)

if(DEFINED FSR3_VULKAN_HEADERS AND NOT FSR3_VULKAN_HEADERS STREQUAL "")
    include_directories("${FSR3_VULKAN_HEADERS}")
endif()

if(FSR3_USE_PREBUILT)
    set(FSR3_PREBUILT_DIR "${CMAKE_CURRENT_SOURCE_DIR}/bin/ffx_sdk")
    foreach(ffx_target
            ffx_backend_vk_android_arm64
            ffx_fsr3_android_arm64
            ffx_fsr3upscaler_android_arm64
            ffx_frameinterpolation_android_arm64
            ffx_opticalflow_android_arm64)
        add_library(${ffx_target} STATIC IMPORTED)
        set_target_properties(${ffx_target} PROPERTIES
            IMPORTED_LOCATION "${FSR3_PREBUILT_DIR}/lib${ffx_target}.a")
    endforeach()
else()
    if(NOT DEFINED FSR3_SC_EXECUTABLE)
        message(FATAL_ERROR
            "FSR3_SC_EXECUTABLE is required when FSR3_USE_PREBUILT=OFF")
    endif()

    set(FFX_API_BACKEND VK_ANDROID_ARM64 CACHE STRING "" FORCE)
    set(FFX_FSR3 ON CACHE BOOL "" FORCE)
    set(FFX_AUTO_COMPILE_SHADERS ON CACHE BOOL "" FORCE)
    add_subdirectory(${FSR3_SDK_ROOT} ${CMAKE_CURRENT_BINARY_DIR}/ffx_sdk)
endif()

add_library(dsfsr3bridge SHARED ds_fsr3_bridge.cpp)
target_include_directories(dsfsr3bridge PRIVATE
    ${FSR3_SDK_ROOT}/include
    ${FSR3_SDK_ROOT}/libs)
target_link_libraries(dsfsr3bridge
    ffx_backend_vk_android_arm64
    ffx_fsr3_android_arm64
    ffx_fsr3upscaler_android_arm64
    ffx_frameinterpolation_android_arm64
    ffx_opticalflow_android_arm64
    vulkan
    android
    log)
