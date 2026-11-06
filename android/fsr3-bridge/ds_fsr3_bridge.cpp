#include <android/log.h>
#include <android/hardware_buffer.h>

#define VK_USE_PLATFORM_ANDROID_KHR
#include <vulkan/vulkan.h>

#include <FidelityFX/host/ffx_fsr3.h>
#include <FidelityFX/host/backends/vk/ffx_vk.h>

#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <ctime>
#include <vector>

#define LOGI(...) __android_log_print(ANDROID_LOG_INFO, "DSFSR3Bridge", __VA_ARGS__)
#define LOGE(...) __android_log_print(ANDROID_LOG_ERROR, "DSFSR3Bridge", __VA_ARGS__)

struct VulkanState {
    VkInstance instance = VK_NULL_HANDLE;
    VkPhysicalDevice physical = VK_NULL_HANDLE;
    VkDevice device = VK_NULL_HANDLE;
    VkQueue queue = VK_NULL_HANDLE;
    VkCommandPool command_pool = VK_NULL_HANDLE;
    uint32_t family = UINT32_MAX;
};

struct UpscaleSession {
    bool valid = false;
    VulkanState vk;

    AHardwareBuffer* input_ahb = nullptr;
    AHardwareBuffer* output_ahbs[2] = {};
    AHardwareBuffer* depth_ahb = nullptr;
    uint32_t input_width = 0;
    uint32_t input_height = 0;
    uint32_t output_width = 0;
    uint32_t output_height = 0;

    VkImageCreateInfo input_info = {VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO};
    VkImageCreateInfo output_ahb_infos[2] = {
        {VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO}, {VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO}};
    VkImageCreateInfo depth_info = {VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO};
    VkImageCreateInfo motion_info = {VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO};
    VkImageCreateInfo output_info = {VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO};
    VkImageCreateInfo fg_output_info = {VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO};

    VkImage input_image = VK_NULL_HANDLE;
    VkDeviceMemory input_memory = VK_NULL_HANDLE;
    VkImage output_ahb_images[2] = {VK_NULL_HANDLE, VK_NULL_HANDLE};
    VkDeviceMemory output_ahb_memories[2] = {VK_NULL_HANDLE, VK_NULL_HANDLE};
    VkImage depth_image = VK_NULL_HANDLE;
    VkDeviceMemory depth_memory = VK_NULL_HANDLE;
    VkImage motion_image = VK_NULL_HANDLE;
    VkDeviceMemory motion_memory = VK_NULL_HANDLE;
    VkImage output_image = VK_NULL_HANDLE;
    VkDeviceMemory output_memory = VK_NULL_HANDLE;
    VkImage fg_output_image = VK_NULL_HANDLE;
    VkDeviceMemory fg_output_memory = VK_NULL_HANDLE;

    FfxInterface iface_shared = {};
    FfxInterface iface_upscaling = {};
    FfxInterface iface_frame = {};
    VkDeviceContext device_context = {};
    std::vector<uint8_t> scratch_shared;
    std::vector<uint8_t> scratch_upscaling;
    std::vector<uint8_t> scratch_frame;
    FfxFsr3Context* context = nullptr;
    bool context_created = false;
    bool frame_generation = false;
    bool synthetic_motion = false;
    bool diagnostic_readback = false;
    bool trace = false;
    bool async_enabled = false;
    bool direct_output = false;
    bool fence_pending = false;
    int last_submitted_output_index = -1;
    int last_ready_output_index = -1;

    VkCommandBuffer command_buffer = VK_NULL_HANDLE;
    VkFence fence = VK_NULL_HANDLE;
    VkBuffer readback_buffer = VK_NULL_HANDLE;
    VkDeviceMemory readback_memory = VK_NULL_HANDLE;
    void* readback_mapped = nullptr;
    VkBuffer depth_staging_buffer = VK_NULL_HANDLE;
    VkDeviceMemory depth_staging_memory = VK_NULL_HANDLE;
    void* depth_staging_mapped = nullptr;

    VkImageLayout input_layout = VK_IMAGE_LAYOUT_UNDEFINED;
    VkImageLayout output_ahb_layouts[2] = {
        VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_UNDEFINED};
    VkImageLayout depth_layout = VK_IMAGE_LAYOUT_UNDEFINED;
    VkImageLayout motion_layout = VK_IMAGE_LAYOUT_UNDEFINED;
    VkImageLayout output_layout = VK_IMAGE_LAYOUT_UNDEFINED;
    VkImageLayout fg_output_layout = VK_IMAGE_LAYOUT_UNDEFINED;
    uint64_t frame_id = 0;
};

static UpscaleSession g_session;
static uint64_t g_dispatch_timing_calls;
static double g_dispatch_timing_total_ms;
static uint64_t g_async_wait_calls;
static double g_async_wait_total_ms;
static uint64_t g_async_submit_calls;
static double g_async_submit_total_ms;

static bool env_flag_enabled(const char* name) {
    const char* value = std::getenv(name);
    return value != nullptr && std::strcmp(value, "1") == 0;
}

static uint32_t find_memory_type(VkPhysicalDevice gpu, uint32_t bits, VkMemoryPropertyFlags wanted) {
    VkPhysicalDeviceMemoryProperties props = {};
    vkGetPhysicalDeviceMemoryProperties(gpu, &props);
    for (uint32_t i = 0; i < props.memoryTypeCount; ++i) {
        if ((bits & (1u << i)) && (props.memoryTypes[i].propertyFlags & wanted) == wanted) return i;
    }
    return UINT32_MAX;
}

static bool create_image(VulkanState& vk, VkImageCreateInfo& info, VkImage& image,
                         VkDeviceMemory& memory,
                         VkAndroidHardwareBufferPropertiesANDROID* ahb_props = nullptr,
                         AHardwareBuffer* ahb = nullptr, const char* label = "image",
                         bool external = false) {
    VkResult result = vkCreateImage(vk.device, &info, nullptr, &image);
    if (result != VK_SUCCESS) {
        LOGE("vkCreateImage failed label=%s result=%d", label, result);
        return false;
    }
    VkMemoryRequirements requirements = {};
    if (external && ahb_props) {
        requirements.size = ahb_props->allocationSize;
        requirements.memoryTypeBits = ahb_props->memoryTypeBits;
    } else {
        vkGetImageMemoryRequirements(vk.device, image, &requirements);
    }
    VkMemoryAllocateInfo alloc = {VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO};
    alloc.allocationSize = requirements.size;
    const VkMemoryPropertyFlags wanted = external ? 0 : VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT;
    alloc.memoryTypeIndex = find_memory_type(vk.physical, requirements.memoryTypeBits, wanted);
    if (alloc.memoryTypeIndex == UINT32_MAX) {
        LOGE("no memory type label=%s bits=0x%x wanted=0x%x", label,
             requirements.memoryTypeBits, wanted);
        vkDestroyImage(vk.device, image, nullptr);
        image = VK_NULL_HANDLE;
        return false;
    }
    VkMemoryDedicatedAllocateInfo dedicated = {VK_STRUCTURE_TYPE_MEMORY_DEDICATED_ALLOCATE_INFO};
    VkImportAndroidHardwareBufferInfoANDROID import_info = {
        VK_STRUCTURE_TYPE_IMPORT_ANDROID_HARDWARE_BUFFER_INFO_ANDROID};
    if (external) {
        dedicated.image = image;
        import_info.buffer = ahb;
        import_info.pNext = &dedicated;
        alloc.pNext = &import_info;
    }
    result = vkAllocateMemory(vk.device, &alloc, nullptr, &memory);
    if (result != VK_SUCCESS) {
        LOGE("vkAllocateMemory failed label=%s result=%d type=%u bits=0x%x size=%llu",
             label, result, alloc.memoryTypeIndex, requirements.memoryTypeBits,
             (unsigned long long)alloc.allocationSize);
        vkDestroyImage(vk.device, image, nullptr);
        image = VK_NULL_HANDLE;
        return false;
    }
    result = vkBindImageMemory(vk.device, image, memory, 0);
    if (result != VK_SUCCESS) {
        LOGE("vkBindImageMemory failed label=%s result=%d", label, result);
        vkFreeMemory(vk.device, memory, nullptr);
        memory = VK_NULL_HANDLE;
        vkDestroyImage(vk.device, image, nullptr);
        image = VK_NULL_HANDLE;
        return false;
    }
    return true;
}

static bool create_device(VulkanState& vk) {
    VkApplicationInfo app = {VK_STRUCTURE_TYPE_APPLICATION_INFO};
    app.pApplicationName = "DeadSpace FSR3 bridge";
    app.apiVersion = VK_API_VERSION_1_3;
    VkInstanceCreateInfo instance_info = {VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO};
    instance_info.pApplicationInfo = &app;
    if (vkCreateInstance(&instance_info, nullptr, &vk.instance) != VK_SUCCESS) return false;

    uint32_t gpu_count = 0;
    vkEnumeratePhysicalDevices(vk.instance, &gpu_count, nullptr);
    std::vector<VkPhysicalDevice> gpus(gpu_count);
    vkEnumeratePhysicalDevices(vk.instance, &gpu_count, gpus.data());
    vk.physical = gpus.empty() ? VK_NULL_HANDLE : gpus[0];
    if (!vk.physical) return false;

    uint32_t family_count = 0;
    vkGetPhysicalDeviceQueueFamilyProperties(vk.physical, &family_count, nullptr);
    std::vector<VkQueueFamilyProperties> families(family_count);
    vkGetPhysicalDeviceQueueFamilyProperties(vk.physical, &family_count, families.data());
    for (uint32_t i = 0; i < family_count; ++i) {
        if ((families[i].queueFlags & (VK_QUEUE_GRAPHICS_BIT | VK_QUEUE_COMPUTE_BIT)) ==
            (VK_QUEUE_GRAPHICS_BIT | VK_QUEUE_COMPUTE_BIT)) {
            vk.family = i;
            break;
        }
    }
    if (vk.family == UINT32_MAX) return false;

    float priority = 1.0f;
    VkDeviceQueueCreateInfo queue_info = {VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO};
    queue_info.queueFamilyIndex = vk.family;
    queue_info.queueCount = 1;
    queue_info.pQueuePriorities = &priority;
    const char* extensions[] = {
        "VK_ANDROID_external_memory_android_hardware_buffer",
        VK_KHR_EXTERNAL_MEMORY_EXTENSION_NAME,
        VK_KHR_DEDICATED_ALLOCATION_EXTENSION_NAME,
        VK_KHR_SYNCHRONIZATION_2_EXTENSION_NAME,
        VK_KHR_SHADER_FLOAT16_INT8_EXTENSION_NAME,
        VK_KHR_GET_MEMORY_REQUIREMENTS_2_EXTENSION_NAME,
    };
    VkDeviceCreateInfo device_info = {VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO};
    device_info.queueCreateInfoCount = 1;
    device_info.pQueueCreateInfos = &queue_info;
    device_info.enabledExtensionCount = (uint32_t)(sizeof(extensions) / sizeof(extensions[0]));
    device_info.ppEnabledExtensionNames = extensions;
    if (vkCreateDevice(vk.physical, &device_info, nullptr, &vk.device) != VK_SUCCESS) return false;
    vkGetDeviceQueue(vk.device, vk.family, 0, &vk.queue);

    VkCommandPoolCreateInfo pool_info = {VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO};
    pool_info.queueFamilyIndex = vk.family;
    pool_info.flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT;
    if (vkCreateCommandPool(vk.device, &pool_info, nullptr, &vk.command_pool) != VK_SUCCESS) return false;
    return true;
}

static VkImageCreateInfo make_image_info(uint32_t w, uint32_t h, VkFormat format,
                                         VkImageUsageFlags usage) {
    VkImageCreateInfo info = {VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO};
    info.imageType = VK_IMAGE_TYPE_2D;
    info.format = format;
    info.extent = {w, h, 1};
    info.mipLevels = 1;
    info.arrayLayers = 1;
    info.samples = VK_SAMPLE_COUNT_1_BIT;
    info.tiling = VK_IMAGE_TILING_OPTIMAL;
    info.usage = usage;
    info.sharingMode = VK_SHARING_MODE_EXCLUSIVE;
    info.initialLayout = VK_IMAGE_LAYOUT_UNDEFINED;
    return info;
}

static void image_barrier(VkCommandBuffer cmd, VkImage image, VkImageLayout old_layout,
                          VkImageLayout new_layout, VkImageAspectFlags aspect,
                          VkAccessFlags src_access, VkAccessFlags dst_access,
                          VkPipelineStageFlags src_stage, VkPipelineStageFlags dst_stage) {
    VkImageMemoryBarrier barrier = {VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER};
    barrier.srcAccessMask = src_access;
    barrier.dstAccessMask = dst_access;
    barrier.oldLayout = old_layout;
    barrier.newLayout = new_layout;
    barrier.srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    barrier.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    barrier.image = image;
    barrier.subresourceRange = {aspect, 0, 1, 0, 1};
    vkCmdPipelineBarrier(cmd, src_stage, dst_stage, 0, 0, nullptr, 0, nullptr, 1, &barrier);
}

static void destroy_session() {
    UpscaleSession& s = g_session;
    if (s.vk.device != VK_NULL_HANDLE) {
        vkDeviceWaitIdle(s.vk.device);
    }
    if (s.context != nullptr) {
        if (s.context_created) ffxFsr3ContextDestroy(s.context);
        delete s.context;
    }
    if (s.readback_mapped != nullptr && s.vk.device != VK_NULL_HANDLE) {
        vkUnmapMemory(s.vk.device, s.readback_memory);
    }
    if (s.depth_staging_mapped != nullptr && s.vk.device != VK_NULL_HANDLE) {
        vkUnmapMemory(s.vk.device, s.depth_staging_memory);
    }
    if (s.vk.device != VK_NULL_HANDLE) {
        if (s.readback_buffer != VK_NULL_HANDLE) vkDestroyBuffer(s.vk.device, s.readback_buffer, nullptr);
        if (s.readback_memory != VK_NULL_HANDLE) vkFreeMemory(s.vk.device, s.readback_memory, nullptr);
        if (s.depth_staging_buffer != VK_NULL_HANDLE) vkDestroyBuffer(s.vk.device, s.depth_staging_buffer, nullptr);
        if (s.depth_staging_memory != VK_NULL_HANDLE) vkFreeMemory(s.vk.device, s.depth_staging_memory, nullptr);
        if (s.fence != VK_NULL_HANDLE) vkDestroyFence(s.vk.device, s.fence, nullptr);
        if (s.output_image != VK_NULL_HANDLE) vkDestroyImage(s.vk.device, s.output_image, nullptr);
        if (s.output_memory != VK_NULL_HANDLE) vkFreeMemory(s.vk.device, s.output_memory, nullptr);
        if (s.fg_output_image != VK_NULL_HANDLE) vkDestroyImage(s.vk.device, s.fg_output_image, nullptr);
        if (s.fg_output_memory != VK_NULL_HANDLE) vkFreeMemory(s.vk.device, s.fg_output_memory, nullptr);
        if (s.motion_image != VK_NULL_HANDLE) vkDestroyImage(s.vk.device, s.motion_image, nullptr);
        if (s.motion_memory != VK_NULL_HANDLE) vkFreeMemory(s.vk.device, s.motion_memory, nullptr);
        if (s.depth_image != VK_NULL_HANDLE) vkDestroyImage(s.vk.device, s.depth_image, nullptr);
        if (s.depth_memory != VK_NULL_HANDLE) vkFreeMemory(s.vk.device, s.depth_memory, nullptr);
        if (s.input_image != VK_NULL_HANDLE) vkDestroyImage(s.vk.device, s.input_image, nullptr);
        if (s.input_memory != VK_NULL_HANDLE) vkFreeMemory(s.vk.device, s.input_memory, nullptr);
        for (int i = 0; i < 2; ++i) {
            if (s.output_ahb_images[i] != VK_NULL_HANDLE)
                vkDestroyImage(s.vk.device, s.output_ahb_images[i], nullptr);
            if (s.output_ahb_memories[i] != VK_NULL_HANDLE)
                vkFreeMemory(s.vk.device, s.output_ahb_memories[i], nullptr);
        }
        if (s.vk.command_pool != VK_NULL_HANDLE) vkDestroyCommandPool(s.vk.device, s.vk.command_pool, nullptr);
        vkDestroyDevice(s.vk.device, nullptr);
    }
    if (s.vk.instance != VK_NULL_HANDLE) {
        vkDestroyInstance(s.vk.instance, nullptr);
    }
    s = UpscaleSession{};
}

static bool create_session(AHardwareBuffer* ahb, AHardwareBuffer* output_ahb,
                           AHardwareBuffer* depth_ahb,
                           uint32_t input_width, uint32_t input_height,
                           uint32_t output_width, uint32_t output_height) {
    UpscaleSession& s = g_session;
    s = UpscaleSession{};
    s.input_ahb = ahb;
    (void)output_ahb;
    s.depth_ahb = depth_ahb;
    s.input_width = input_width;
    s.input_height = input_height;
    s.output_width = output_width;
    s.output_height = output_height;
    const char* synth_env = std::getenv("TOUCHHLE_DS_FSR3_FG_SYNTHETIC");
    s.synthetic_motion = synth_env != nullptr && std::strcmp(synth_env, "1") == 0;
    const char* fg_env = std::getenv("TOUCHHLE_DS_FSR3_FG");
    const bool frame_generation_requested =
        fg_env != nullptr && std::strcmp(fg_env, "1") == 0;
    const char* fg_probe_env = std::getenv("TOUCHHLE_DS_FSR3_FG_PROBE");
    const bool frame_generation_probe =
        fg_probe_env != nullptr && std::strcmp(fg_probe_env, "1") == 0;
    // A color-only game frame has no trustworthy per-pixel motion. Keep the
    // synthetic probe available for diagnostics, but reject the normal FG
    // toggle until a real motion source is wired in.
    s.frame_generation =
        frame_generation_requested && (s.synthetic_motion || frame_generation_probe);
    if (frame_generation_requested && !s.frame_generation) {
        LOGI("frame generation disabled: no validated motion source");
    }
    const char* diag_env = std::getenv("TOUCHHLE_DS_FSR3_DIAG_READBACK");
    s.diagnostic_readback = diag_env != nullptr && std::strcmp(diag_env, "1") == 0;
    const char* trace_env = std::getenv("TOUCHHLE_DS_FSR3_TRACE");
    s.trace = trace_env != nullptr && std::strcmp(trace_env, "1") == 0;
    const bool async_requested = env_flag_enabled("TOUCHHLE_DS_FSR3_ASYNC");
    s.async_enabled =
        async_requested && !s.diagnostic_readback && !s.frame_generation;
    if (async_requested && !s.async_enabled) {
        LOGI("async FSR pipeline disabled for diagnostic readback or frame generation");
    }
    s.direct_output =
        s.async_enabled && env_flag_enabled("TOUCHHLE_DS_FSR3_DIRECT_OUTPUT");
    if (s.frame_generation) {
        LOGI("frame generation enabled for persistent FSR3 session");
    }

    if (!create_device(s.vk)) {
        LOGE("persistent Vulkan device creation failed");
        destroy_session();
        return false;
    }

    PFN_vkGetAndroidHardwareBufferPropertiesANDROID get_ahb_props =
        (PFN_vkGetAndroidHardwareBufferPropertiesANDROID)vkGetDeviceProcAddr(
            s.vk.device, "vkGetAndroidHardwareBufferPropertiesANDROID");
    if (!get_ahb_props) {
        LOGE("AHB properties entry point missing");
        destroy_session();
        return false;
    }

    VkAndroidHardwareBufferPropertiesANDROID ahb_props = {
        VK_STRUCTURE_TYPE_ANDROID_HARDWARE_BUFFER_PROPERTIES_ANDROID};
    VkAndroidHardwareBufferFormatPropertiesANDROID ahb_format = {
        VK_STRUCTURE_TYPE_ANDROID_HARDWARE_BUFFER_FORMAT_PROPERTIES_ANDROID};
    ahb_props.pNext = &ahb_format;
    if (get_ahb_props(s.vk.device, ahb, &ahb_props) != VK_SUCCESS) {
        LOGE("getAHBProperties failed");
        destroy_session();
        return false;
    }

    VkExternalMemoryImageCreateInfo input_ext = {
        VK_STRUCTURE_TYPE_EXTERNAL_MEMORY_IMAGE_CREATE_INFO};
    input_ext.handleTypes = VK_EXTERNAL_MEMORY_HANDLE_TYPE_ANDROID_HARDWARE_BUFFER_BIT_ANDROID;
    s.input_info = {VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO};
    s.input_info.pNext = &input_ext;
    s.input_info.imageType = VK_IMAGE_TYPE_2D;
    s.input_info.format = ahb_format.format == VK_FORMAT_UNDEFINED
        ? VK_FORMAT_R8G8B8A8_UNORM : ahb_format.format;
    s.input_info.extent = {input_width, input_height, 1};
    s.input_info.mipLevels = 1;
    s.input_info.arrayLayers = 1;
    s.input_info.samples = VK_SAMPLE_COUNT_1_BIT;
    s.input_info.tiling = VK_IMAGE_TILING_OPTIMAL;
    s.input_info.usage = VK_IMAGE_USAGE_SAMPLED_BIT | VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT;
    s.input_info.sharingMode = VK_SHARING_MODE_EXCLUSIVE;
    s.input_info.initialLayout = VK_IMAGE_LAYOUT_UNDEFINED;
    if (!create_image(s.vk, s.input_info, s.input_image, s.input_memory,
                      &ahb_props, ahb, "input", true)) {
        destroy_session();
        return false;
    }
    s.input_info.pNext = nullptr;

    if (s.depth_ahb != nullptr) {
        VkAndroidHardwareBufferPropertiesANDROID depth_props = {
            VK_STRUCTURE_TYPE_ANDROID_HARDWARE_BUFFER_PROPERTIES_ANDROID};
        VkAndroidHardwareBufferFormatPropertiesANDROID depth_format = {
            VK_STRUCTURE_TYPE_ANDROID_HARDWARE_BUFFER_FORMAT_PROPERTIES_ANDROID};
        depth_props.pNext = &depth_format;
        if (get_ahb_props(s.vk.device, s.depth_ahb, &depth_props) != VK_SUCCESS) {
            LOGE("getDepthAHBProperties failed");
            destroy_session();
            return false;
        }
        VkExternalMemoryImageCreateInfo depth_ext = {
            VK_STRUCTURE_TYPE_EXTERNAL_MEMORY_IMAGE_CREATE_INFO};
        depth_ext.handleTypes =
            VK_EXTERNAL_MEMORY_HANDLE_TYPE_ANDROID_HARDWARE_BUFFER_BIT_ANDROID;
        const VkFormat depth_format_value = depth_format.format == VK_FORMAT_UNDEFINED
            ? VK_FORMAT_D16_UNORM : depth_format.format;
        s.depth_info = make_image_info(
            input_width, input_height, depth_format_value, VK_IMAGE_USAGE_SAMPLED_BIT);
        s.depth_info.pNext = &depth_ext;
        if (!create_image(s.vk, s.depth_info, s.depth_image, s.depth_memory,
                          &depth_props, s.depth_ahb, "depth_ahb", true)) {
            destroy_session();
            return false;
        }
        s.depth_info.pNext = nullptr;
        LOGI("external depth AHB imported: format=%d", (int)depth_format_value);
    } else {
        s.depth_info = make_image_info(input_width, input_height, VK_FORMAT_D32_SFLOAT,
            VK_IMAGE_USAGE_DEPTH_STENCIL_ATTACHMENT_BIT | VK_IMAGE_USAGE_SAMPLED_BIT |
            VK_IMAGE_USAGE_TRANSFER_DST_BIT);
        if (!create_image(s.vk, s.depth_info, s.depth_image, s.depth_memory,
                          nullptr, nullptr, "depth")) {
            destroy_session();
            return false;
        }
    }

    s.motion_info = make_image_info(input_width, input_height, VK_FORMAT_R16G16_SFLOAT,
        VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT | VK_IMAGE_USAGE_SAMPLED_BIT |
        VK_IMAGE_USAGE_TRANSFER_DST_BIT);
    if (!create_image(s.vk, s.motion_info, s.motion_image, s.motion_memory,
                      nullptr, nullptr, "motion")) {
        destroy_session();
        return false;
    }

    s.output_info = make_image_info(output_width, output_height, VK_FORMAT_R8G8B8A8_UNORM,
        VK_IMAGE_USAGE_STORAGE_BIT | VK_IMAGE_USAGE_SAMPLED_BIT |
        VK_IMAGE_USAGE_TRANSFER_SRC_BIT);
    if (!s.direct_output) {
        if (!create_image(s.vk, s.output_info, s.output_image, s.output_memory,
                          nullptr, nullptr, "output")) {
            destroy_session();
            return false;
        }
    }
    if (s.frame_generation) {
        s.fg_output_info = make_image_info(output_width, output_height, VK_FORMAT_R8G8B8A8_UNORM,
            VK_IMAGE_USAGE_STORAGE_BIT | VK_IMAGE_USAGE_SAMPLED_BIT |
            VK_IMAGE_USAGE_TRANSFER_SRC_BIT);
        if (!create_image(s.vk, s.fg_output_info, s.fg_output_image, s.fg_output_memory,
                          nullptr, nullptr, "fg_output")) {
            destroy_session();
            return false;
        }
    }

    s.scratch_shared.resize(ffxGetScratchMemorySizeVK(s.vk.physical, 2));
    s.scratch_upscaling.resize(ffxGetScratchMemorySizeVK(s.vk.physical, 2));
    s.scratch_frame.resize(ffxGetScratchMemorySizeVK(s.vk.physical, 3));
    s.device_context = {s.vk.device, s.vk.physical, vkGetDeviceProcAddr};
    FfxDevice ffx_device = ffxGetDeviceVK(&s.device_context);
    FfxErrorCode error = ffxGetInterfaceVK(&s.iface_shared, ffx_device,
        s.scratch_shared.data(), s.scratch_shared.size(), 2);
    if (error != FFX_OK) {
        LOGE("ffxGetInterfaceVK(shared) failed: %d", (int)error);
        destroy_session();
        return false;
    }
    error = ffxGetInterfaceVK(&s.iface_upscaling, ffx_device,
        s.scratch_upscaling.data(), s.scratch_upscaling.size(), 2);
    if (error != FFX_OK) {
        LOGE("ffxGetInterfaceVK(upscaling) failed: %d", (int)error);
        destroy_session();
        return false;
    }
    error = ffxGetInterfaceVK(&s.iface_frame, ffx_device,
        s.scratch_frame.data(), s.scratch_frame.size(), 3);
    if (error != FFX_OK) {
        LOGE("ffxGetInterfaceVK(frame) failed: %d", (int)error);
        destroy_session();
        return false;
    }

    s.context = new FfxFsr3Context{};
    FfxFsr3ContextDescription desc = {};
    desc.flags = FFX_FSR3_ENABLE_AUTO_EXPOSURE;
    if (!s.frame_generation) desc.flags |= FFX_FSR3_ENABLE_UPSCALING_ONLY;
    desc.maxRenderSize = {input_width, input_height};
    desc.maxUpscaleSize = {output_width, output_height};
    desc.displaySize = {output_width, output_height};
    desc.backendInterfaceSharedResources = s.iface_shared;
    desc.backendInterfaceUpscaling = s.iface_upscaling;
    desc.backendInterfaceFrameInterpolation = s.iface_frame;
    desc.backBufferFormat = FFX_SURFACE_FORMAT_R8G8B8A8_UNORM;
    error = ffxFsr3ContextCreate(s.context, &desc);
    if (error != FFX_OK) {
        LOGE("ffxFsr3ContextCreate failed: %d", (int)error);
        destroy_session();
        return false;
    }
    s.context_created = true;
    if (s.frame_generation) {
        // Configure the context for manual frame-generation dispatch. The
        // Vulkan backend expects a swapchain handle here and returns
        // FFX_ERROR_INVALID_ARGUMENT for null, but the FSR3 context still
        // records frameGenerationEnabled and publishes its global context
        // pointer before that return. We manage presentation ourselves.
        FfxFrameGenerationConfig config = {};
        config.frameGenerationEnabled = true;
        config.allowAsyncWorkloads = false;
        config.swapChain = nullptr;
        config.flags = 0;
        FfxErrorCode cfg = ffxFsr3ConfigureFrameGeneration(s.context, &config);
        LOGI("manual frame generation configured result=%d", (int)cfg);
    }

    VkCommandBufferAllocateInfo cmd_alloc = {VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO};
    cmd_alloc.commandPool = s.vk.command_pool;
    cmd_alloc.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY;
    cmd_alloc.commandBufferCount = 1;
    if (vkAllocateCommandBuffers(s.vk.device, &cmd_alloc, &s.command_buffer) != VK_SUCCESS) {
        destroy_session();
        return false;
    }

    VkFenceCreateInfo fence_info = {VK_STRUCTURE_TYPE_FENCE_CREATE_INFO};
    fence_info.flags = VK_FENCE_CREATE_SIGNALED_BIT;
    if (vkCreateFence(s.vk.device, &fence_info, nullptr, &s.fence) != VK_SUCCESS) {
        destroy_session();
        return false;
    }

    if (s.diagnostic_readback) {
        VkBufferCreateInfo readback_info = {VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO};
        readback_info.size = 8;
        readback_info.usage = VK_BUFFER_USAGE_TRANSFER_DST_BIT;
        if (vkCreateBuffer(s.vk.device, &readback_info, nullptr, &s.readback_buffer) != VK_SUCCESS) {
            destroy_session();
            return false;
        }
        VkMemoryRequirements readback_req = {};
        vkGetBufferMemoryRequirements(s.vk.device, s.readback_buffer, &readback_req);
        VkMemoryAllocateInfo readback_alloc = {VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO};
        readback_alloc.allocationSize = readback_req.size;
        readback_alloc.memoryTypeIndex = find_memory_type(s.vk.physical, readback_req.memoryTypeBits,
            VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT);
        if (readback_alloc.memoryTypeIndex == UINT32_MAX ||
            vkAllocateMemory(s.vk.device, &readback_alloc, nullptr, &s.readback_memory) != VK_SUCCESS ||
            vkBindBufferMemory(s.vk.device, s.readback_buffer, s.readback_memory, 0) != VK_SUCCESS ||
            vkMapMemory(s.vk.device, s.readback_memory, 0, VK_WHOLE_SIZE, 0, &s.readback_mapped) != VK_SUCCESS) {
            LOGE("persistent center readback setup failed");
            destroy_session();
            return false;
        }
    }

    VkBufferCreateInfo depth_staging_info = {VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO};
    depth_staging_info.size = (VkDeviceSize)input_width * input_height * sizeof(float);
    depth_staging_info.usage = VK_BUFFER_USAGE_TRANSFER_SRC_BIT;
    if (vkCreateBuffer(s.vk.device, &depth_staging_info, nullptr, &s.depth_staging_buffer) != VK_SUCCESS) {
        destroy_session();
        return false;
    }
    VkMemoryRequirements depth_staging_req = {};
    vkGetBufferMemoryRequirements(s.vk.device, s.depth_staging_buffer, &depth_staging_req);
    VkMemoryAllocateInfo depth_staging_alloc = {VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO};
    depth_staging_alloc.allocationSize = depth_staging_req.size;
    depth_staging_alloc.memoryTypeIndex = find_memory_type(s.vk.physical, depth_staging_req.memoryTypeBits,
        VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT);
    if (depth_staging_alloc.memoryTypeIndex == UINT32_MAX ||
        vkAllocateMemory(s.vk.device, &depth_staging_alloc, nullptr, &s.depth_staging_memory) != VK_SUCCESS ||
        vkBindBufferMemory(s.vk.device, s.depth_staging_buffer, s.depth_staging_memory, 0) != VK_SUCCESS ||
        vkMapMemory(s.vk.device, s.depth_staging_memory, 0, VK_WHOLE_SIZE, 0, &s.depth_staging_mapped) != VK_SUCCESS) {
        LOGE("depth staging setup failed");
        destroy_session();
        return false;
    }

    s.valid = true;
    LOGI("persistent session initialized: FSR%d.%d.%d %ux%u -> %ux%u "
         "frameGeneration=%d syntheticMotion=%d diagnosticReadback=%d async=%d direct=%d",
         FFX_FSR3_VERSION_MAJOR, FFX_FSR3_VERSION_MINOR, FFX_FSR3_VERSION_PATCH,
         input_width, input_height, output_width, output_height,
         s.frame_generation ? 1 : 0, s.synthetic_motion ? 1 : 0,
         s.diagnostic_readback ? 1 : 0, s.async_enabled ? 1 : 0,
         s.direct_output ? 1 : 0);
    return true;
}

static int ensure_output_ahb(AHardwareBuffer* output_ahb) {
    UpscaleSession& s = g_session;
    for (int i = 0; i < 2; ++i) {
        if (s.output_ahbs[i] == output_ahb) return i;
    }
    for (int i = 0; i < 2; ++i) {
        if (s.output_ahbs[i] != nullptr) continue;
        PFN_vkGetAndroidHardwareBufferPropertiesANDROID get_ahb_props =
            (PFN_vkGetAndroidHardwareBufferPropertiesANDROID)vkGetDeviceProcAddr(
                s.vk.device, "vkGetAndroidHardwareBufferPropertiesANDROID");
        if (!get_ahb_props) return -1;
        VkAndroidHardwareBufferPropertiesANDROID props = {
            VK_STRUCTURE_TYPE_ANDROID_HARDWARE_BUFFER_PROPERTIES_ANDROID};
        VkAndroidHardwareBufferFormatPropertiesANDROID format = {
            VK_STRUCTURE_TYPE_ANDROID_HARDWARE_BUFFER_FORMAT_PROPERTIES_ANDROID};
        props.pNext = &format;
        if (get_ahb_props(s.vk.device, output_ahb, &props) != VK_SUCCESS) {
            LOGE("getOutputAHBProperties failed index=%d", i);
            return -1;
        }
        VkExternalMemoryImageCreateInfo ext = {
            VK_STRUCTURE_TYPE_EXTERNAL_MEMORY_IMAGE_CREATE_INFO};
        ext.handleTypes = VK_EXTERNAL_MEMORY_HANDLE_TYPE_ANDROID_HARDWARE_BUFFER_BIT_ANDROID;
        s.output_ahb_infos[i] = {VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO};
        s.output_ahb_infos[i].pNext = &ext;
        s.output_ahb_infos[i].imageType = VK_IMAGE_TYPE_2D;
        s.output_ahb_infos[i].format = format.format == VK_FORMAT_UNDEFINED
            ? VK_FORMAT_R8G8B8A8_UNORM : format.format;
        s.output_ahb_infos[i].extent = {s.output_width, s.output_height, 1};
        s.output_ahb_infos[i].mipLevels = 1;
        s.output_ahb_infos[i].arrayLayers = 1;
        s.output_ahb_infos[i].samples = VK_SAMPLE_COUNT_1_BIT;
        s.output_ahb_infos[i].tiling = VK_IMAGE_TILING_OPTIMAL;
        s.output_ahb_infos[i].usage =
            VK_IMAGE_USAGE_TRANSFER_DST_BIT | VK_IMAGE_USAGE_SAMPLED_BIT |
            (s.direct_output ? VK_IMAGE_USAGE_STORAGE_BIT : 0);
        s.output_ahb_infos[i].sharingMode = VK_SHARING_MODE_EXCLUSIVE;
        s.output_ahb_infos[i].initialLayout = VK_IMAGE_LAYOUT_UNDEFINED;
        if (!create_image(s.vk, s.output_ahb_infos[i], s.output_ahb_images[i],
                          s.output_ahb_memories[i], &props, output_ahb,
                          "output_ahb", true)) {
            s.output_ahb_infos[i] = {VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO};
            return -1;
        }
        s.output_ahb_infos[i].pNext = nullptr;
        s.output_ahbs[i] = output_ahb;
        LOGI("cached output AHB index=%d handle=%p image=%p", i, (void*)output_ahb,
             (void*)s.output_ahb_images[i]);
        return i;
    }
    LOGE("more than two output AHBs supplied to persistent FSR3 session");
    return -1;
}
static bool ensure_session(AHardwareBuffer* ahb, AHardwareBuffer* output_ahb,
                           AHardwareBuffer* depth_ahb,
                           uint32_t input_width, uint32_t input_height,
                           uint32_t output_width, uint32_t output_height) {
    UpscaleSession& s = g_session;
    if (s.valid && s.input_ahb == ahb && s.depth_ahb == depth_ahb &&
        s.input_width == input_width && s.input_height == input_height &&
        s.output_width == output_width && s.output_height == output_height) {
        return true;
    }
    if (s.valid || s.vk.device != VK_NULL_HANDLE || s.vk.instance != VK_NULL_HANDLE) {
        destroy_session();
    }
    return create_session(
        ahb, output_ahb, depth_ahb, input_width, input_height, output_width, output_height);
}

static bool wait_pending_fence() {
    UpscaleSession& s = g_session;
    if (!s.valid || !s.async_enabled || !s.fence_pending) return true;

    struct timespec wait_start;
    const bool timing_valid = clock_gettime(CLOCK_MONOTONIC, &wait_start) == 0;
    if (vkWaitForFences(s.vk.device, 1, &s.fence, VK_TRUE, UINT64_MAX) != VK_SUCCESS) {
        LOGE("wait for async FSR fence failed frame=%llu",
             (unsigned long long)s.frame_id);
        destroy_session();
        return false;
    }
    s.fence_pending = false;
    s.last_ready_output_index = s.last_submitted_output_index;

    if (timing_valid) {
        struct timespec wait_end;
        if (clock_gettime(CLOCK_MONOTONIC, &wait_end) == 0) {
            const double elapsed_ms =
                (wait_end.tv_sec - wait_start.tv_sec) * 1000.0 +
                (wait_end.tv_nsec - wait_start.tv_nsec) / 1000000.0;
            g_async_wait_total_ms += elapsed_ms;
            g_async_wait_calls++;
            if (g_async_wait_calls == 1 || g_async_wait_calls % 120 == 0) {
                LOGI("FSR async previous-frame wait timing: avg %.2fms over %llu frames",
                     g_async_wait_total_ms / g_async_wait_calls,
                     (unsigned long long)g_async_wait_calls);
            }
        }
    }
    return true;
}

extern "C" AHardwareBuffer* ds_fsr3_wait_pending_ahb() {
    UpscaleSession& s = g_session;
    if (!s.valid || !s.async_enabled || !s.fence_pending) return nullptr;
    if (!wait_pending_fence()) return nullptr;
    if (s.last_ready_output_index < 0 || s.last_ready_output_index >= 2) {
        return nullptr;
    }
    return s.output_ahbs[s.last_ready_output_index];
}

extern "C" bool ds_fsr3_upscale_ahb(
    AHardwareBuffer* ahb,
    AHardwareBuffer* output_ahb,
    AHardwareBuffer* depth_ahb,
    uint32_t input_width,
    uint32_t input_height,
    uint32_t output_width,
    uint32_t output_height,
    const float* depth_data,
    float frame_time_ms,
    uint8_t out_center_rgba[4],
    AHardwareBuffer** out_ready_output_ahb) {
    if (!ahb || !output_ahb || !out_center_rgba) return false;
    if (out_ready_output_ahb != nullptr) *out_ready_output_ahb = nullptr;
    if (!ensure_session(
            ahb, output_ahb, depth_ahb, input_width, input_height, output_width, output_height)) {
        return false;
    }

    struct timespec dispatch_start;
    const bool dispatch_timing_valid = clock_gettime(CLOCK_MONOTONIC, &dispatch_start) == 0;
    UpscaleSession& s = g_session;
    if (depth_data != nullptr && s.depth_ahb == nullptr) {
        const size_t depth_bytes = (size_t)input_width * input_height * sizeof(float);
        memcpy(s.depth_staging_mapped, depth_data, depth_bytes);
    }
    int output_index = ensure_output_ahb(output_ahb);
    if (output_index < 0) return false;
    VkImage output_ahb_image = s.output_ahb_images[output_index];
    VkImageLayout& output_ahb_layout = s.output_ahb_layouts[output_index];
    const bool trace = s.trace && (s.frame_id < 10 || (s.frame_id % 120 == 0));
    if (trace) LOGI("enter completed_frames=%llu", (unsigned long long)s.frame_id);
    if (s.async_enabled && s.fence_pending && !wait_pending_fence()) return false;
    if (!s.async_enabled &&
        vkWaitForFences(s.vk.device, 1, &s.fence, VK_TRUE, UINT64_MAX) != VK_SUCCESS) {
            LOGE("wait for persistent fence failed");
            return false;
        }
    if (trace) LOGI("fence ready completed_frames=%llu", (unsigned long long)s.frame_id);
    vkResetFences(s.vk.device, 1, &s.fence);
    if (vkResetCommandBuffer(s.command_buffer, 0) != VK_SUCCESS) return false;
    VkCommandBufferBeginInfo begin = {VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO};
    begin.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
    if (vkBeginCommandBuffer(s.command_buffer, &begin) != VK_SUCCESS) return false;
    if (trace) LOGI("command buffer begun completed_frames=%llu", (unsigned long long)s.frame_id);
    VkCommandBuffer cmd = s.command_buffer;

    if (s.input_layout != VK_IMAGE_LAYOUT_GENERAL) {
        image_barrier(cmd, s.input_image, s.input_layout, VK_IMAGE_LAYOUT_GENERAL,
                      VK_IMAGE_ASPECT_COLOR_BIT, 0, VK_ACCESS_SHADER_READ_BIT,
                      VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT, VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT);
        s.input_layout = VK_IMAGE_LAYOUT_GENERAL;
    }
    if (s.direct_output && output_ahb_layout != VK_IMAGE_LAYOUT_GENERAL) {
        image_barrier(cmd, output_ahb_image, output_ahb_layout,
                      VK_IMAGE_LAYOUT_GENERAL, VK_IMAGE_ASPECT_COLOR_BIT, 0,
                      VK_ACCESS_SHADER_READ_BIT | VK_ACCESS_SHADER_WRITE_BIT,
                      VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT,
                      VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT);
        output_ahb_layout = VK_IMAGE_LAYOUT_GENERAL;
    } else if (!s.direct_output && s.output_layout != VK_IMAGE_LAYOUT_GENERAL) {
        image_barrier(cmd, s.output_image, s.output_layout, VK_IMAGE_LAYOUT_GENERAL,
                      VK_IMAGE_ASPECT_COLOR_BIT, 0, VK_ACCESS_SHADER_READ_BIT | VK_ACCESS_SHADER_WRITE_BIT,
                      VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT, VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT);
        s.output_layout = VK_IMAGE_LAYOUT_GENERAL;
    }

    if (s.depth_ahb != nullptr) {
        if (s.depth_layout != VK_IMAGE_LAYOUT_GENERAL) {
            image_barrier(cmd, s.depth_image, s.depth_layout, VK_IMAGE_LAYOUT_GENERAL,
                          VK_IMAGE_ASPECT_DEPTH_BIT, 0, VK_ACCESS_SHADER_READ_BIT,
                          VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT,
                          VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT);
            s.depth_layout = VK_IMAGE_LAYOUT_GENERAL;
        }
    } else {
        VkClearDepthStencilValue clear_depth = {1.0f, 0};
        VkImageSubresourceRange depth_range = {VK_IMAGE_ASPECT_DEPTH_BIT, 0, 1, 0, 1};
        image_barrier(cmd, s.depth_image, s.depth_layout, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                      VK_IMAGE_ASPECT_DEPTH_BIT, 0, VK_ACCESS_TRANSFER_WRITE_BIT,
                      VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT);
        if (depth_data != nullptr) {
            VkBufferImageCopy depth_copy = {};
            depth_copy.imageSubresource = {VK_IMAGE_ASPECT_DEPTH_BIT, 0, 0, 1};
            depth_copy.imageExtent = {input_width, input_height, 1};
            vkCmdCopyBufferToImage(cmd, s.depth_staging_buffer, s.depth_image,
                                   VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &depth_copy);
        } else {
            vkCmdClearDepthStencilImage(
                cmd, s.depth_image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                &clear_depth, 1, &depth_range);
        }
        image_barrier(cmd, s.depth_image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                      VK_IMAGE_LAYOUT_GENERAL, VK_IMAGE_ASPECT_DEPTH_BIT,
                      VK_ACCESS_TRANSFER_WRITE_BIT, VK_ACCESS_SHADER_READ_BIT,
                      VK_PIPELINE_STAGE_TRANSFER_BIT,
                      VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT);
        s.depth_layout = VK_IMAGE_LAYOUT_GENERAL;
    }

    VkClearColorValue clear_motion = {};
    if (s.frame_generation && s.synthetic_motion) clear_motion.float32[0] = 4.0f;
    VkImageSubresourceRange motion_range = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    image_barrier(cmd, s.motion_image, s.motion_layout, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                  VK_IMAGE_ASPECT_COLOR_BIT, 0, VK_ACCESS_TRANSFER_WRITE_BIT,
                  VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT);
    vkCmdClearColorImage(cmd, s.motion_image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                         &clear_motion, 1, &motion_range);
    image_barrier(cmd, s.motion_image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_IMAGE_LAYOUT_GENERAL,
                  VK_IMAGE_ASPECT_COLOR_BIT, VK_ACCESS_TRANSFER_WRITE_BIT, VK_ACCESS_SHADER_READ_BIT,
                  VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT);
    s.motion_layout = VK_IMAGE_LAYOUT_GENERAL;

    FfxResource color_res = ffxGetResourceVK(s.input_image,
        ffxGetImageResourceDescriptionVK(s.input_image, s.input_info),
        (wchar_t*)L"DeadSpaceColor", FFX_RESOURCE_STATE_COMPUTE_READ);
    FfxResource depth_res = ffxGetResourceVK(s.depth_image,
        ffxGetImageResourceDescriptionVK(s.depth_image, s.depth_info),
        (wchar_t*)L"DeadSpaceDepth", FFX_RESOURCE_STATE_COMPUTE_READ);
    FfxResource motion_res = ffxGetResourceVK(s.motion_image,
        ffxGetImageResourceDescriptionVK(s.motion_image, s.motion_info),
        (wchar_t*)L"DeadSpaceMotion", FFX_RESOURCE_STATE_COMPUTE_READ);
    VkImage output_resource_image =
        s.direct_output ? output_ahb_image : s.output_image;
    VkImageCreateInfo& output_resource_info =
        s.direct_output ? s.output_ahb_infos[output_index] : s.output_info;
    FfxResource output_res = ffxGetResourceVK(output_resource_image,
        ffxGetImageResourceDescriptionVK(
            output_resource_image, output_resource_info, FFX_RESOURCE_USAGE_UAV),
        (wchar_t*)L"DeadSpaceOutput", FFX_RESOURCE_STATE_UNORDERED_ACCESS);

    FfxFsr3DispatchUpscaleDescription upscale = {};
    upscale.commandList = ffxGetCommandListVK(cmd);
    upscale.color = color_res;
    upscale.depth = depth_res;
    upscale.motionVectors = motion_res;
    upscale.upscaleOutput = output_res;
    upscale.jitterOffset = {0.0f, 0.0f};
    upscale.motionVectorScale = s.synthetic_motion
        ? FfxFloatCoords2D{1.0f, 1.0f} : FfxFloatCoords2D{0.0f, 0.0f};
    upscale.renderSize = {input_width, input_height};
    upscale.upscaleSize = {output_width, output_height};
    upscale.enableSharpening = false;
    upscale.sharpness = 0.0f;
    upscale.frameTimeDelta = frame_time_ms;
    upscale.preExposure = 1.0f;
    upscale.reset = true;
    upscale.cameraNear = 0.1f;
    upscale.cameraFar = 100.0f;
    upscale.cameraFovAngleVertical = 0.7853981633974483f;
    upscale.viewSpaceToMetersFactor = 1.0f;
    upscale.frameID = ++s.frame_id;

    if (trace) LOGI("dispatching completed_frames=%llu", (unsigned long long)s.frame_id);
    FfxErrorCode error = ffxFsr3ContextDispatchUpscale(s.context, &upscale);
    if (trace) LOGI("dispatch returned=%d completed_frames=%llu", (int)error,
                    (unsigned long long)s.frame_id);
    if (error != FFX_OK) {
        LOGE("ffxFsr3ContextDispatchUpscale failed: %d frame=%llu", (int)error,
             (unsigned long long)s.frame_id);
        vkEndCommandBuffer(cmd);
        return false;
    }

    if (s.frame_generation) {
        if (s.fg_output_layout != VK_IMAGE_LAYOUT_GENERAL) {
            image_barrier(cmd, s.fg_output_image, s.fg_output_layout,
                          VK_IMAGE_LAYOUT_GENERAL, VK_IMAGE_ASPECT_COLOR_BIT, 0,
                          VK_ACCESS_SHADER_READ_BIT | VK_ACCESS_SHADER_WRITE_BIT,
                          VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT,
                          VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT);
            s.fg_output_layout = VK_IMAGE_LAYOUT_GENERAL;
        }
        FfxFsr3DispatchFrameGenerationPrepareDescription prepare = {};
        prepare.commandList = ffxGetCommandListVK(cmd);
        prepare.depth = depth_res;
        prepare.motionVectors = motion_res;
        prepare.jitterOffset = {0.0f, 0.0f};
        prepare.motionVectorScale = s.synthetic_motion ? FfxFloatCoords2D{1.0f, 1.0f} : FfxFloatCoords2D{0.0f, 0.0f};
        prepare.renderSize = {input_width, input_height};
        prepare.frameTimeDelta = frame_time_ms;
        prepare.cameraNear = 0.1f;
        prepare.cameraFar = 100.0f;
        prepare.viewSpaceToMetersFactor = 1.0f;
        prepare.cameraFovAngleVertical = 0.7853981633974483f;
        prepare.frameID = s.frame_id;
        error = ffxFsr3ContextDispatchFrameGenerationPrepare(s.context, &prepare);
        if (error != FFX_OK) {
            LOGE("ffxFsr3ContextDispatchFrameGenerationPrepare failed: %d frame=%llu",
                 (int)error, (unsigned long long)s.frame_id);
        } else {
            FfxResource present_res = ffxGetResourceVK(
                s.output_image,
                ffxGetImageResourceDescriptionVK(s.output_image, s.output_info),
                (wchar_t*)L"DeadSpacePresent", FFX_RESOURCE_STATE_COMPUTE_READ);
            FfxResource fg_output_res = ffxGetResourceVK(
                s.fg_output_image,
                ffxGetImageResourceDescriptionVK(s.fg_output_image, s.fg_output_info,
                                                 FFX_RESOURCE_USAGE_UAV),
                (wchar_t*)L"DeadSpaceFrameGeneration",
                FFX_RESOURCE_STATE_UNORDERED_ACCESS);
            FfxFrameGenerationDispatchDescription fg = {};
            fg.commandList = ffxGetCommandListVK(cmd);
            fg.presentColor = present_res;
            fg.outputs[0] = fg_output_res;
            fg.numInterpolatedFrames = 1;
            fg.reset = true;
            fg.backBufferTransferFunction = FFX_BACKBUFFER_TRANSFER_FUNCTION_SRGB;
            fg.minMaxLuminance[0] = 0.0f;
            fg.minMaxLuminance[1] = 1000.0f;
            fg.interpolationRect = {0, 0, (int32_t)output_width, (int32_t)output_height};
            fg.frameID = s.frame_id;
            error = ffxFsr3DispatchFrameGeneration(&fg);
            if (error != FFX_OK) {
                LOGE("ffxFsr3DispatchFrameGeneration failed: %d frame=%llu",
                     (int)error, (unsigned long long)s.frame_id);
            } else {
                image_barrier(cmd, s.fg_output_image, VK_IMAGE_LAYOUT_GENERAL,
                              VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                              VK_IMAGE_ASPECT_COLOR_BIT, VK_ACCESS_SHADER_WRITE_BIT,
                              VK_ACCESS_TRANSFER_READ_BIT,
                              VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT,
                              VK_PIPELINE_STAGE_TRANSFER_BIT);
                s.fg_output_layout = VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL;
            }
        }
    }

    if (!s.direct_output) {
        image_barrier(cmd, s.output_image, VK_IMAGE_LAYOUT_GENERAL,
                      VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_ASPECT_COLOR_BIT,
                      VK_ACCESS_SHADER_WRITE_BIT, VK_ACCESS_TRANSFER_READ_BIT,
                      VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT,
                      VK_PIPELINE_STAGE_TRANSFER_BIT);
        s.output_layout = VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL;

        image_barrier(cmd, output_ahb_image, output_ahb_layout,
                      VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                      VK_IMAGE_ASPECT_COLOR_BIT, 0, VK_ACCESS_TRANSFER_WRITE_BIT,
                      VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT,
                      VK_PIPELINE_STAGE_TRANSFER_BIT);
        VkImageCopy image_copy = {};
        image_copy.srcSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
        image_copy.dstSubresource = image_copy.srcSubresource;
        image_copy.extent = {output_width, output_height, 1};
        VkImage presentation_source = s.output_image;
        if (s.frame_generation && s.synthetic_motion &&
            s.fg_output_layout == VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL) {
            presentation_source = s.fg_output_image;
        }
        vkCmdCopyImage(cmd, presentation_source,
                       VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, output_ahb_image,
                       VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &image_copy);
        image_barrier(cmd, output_ahb_image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                      VK_IMAGE_LAYOUT_GENERAL, VK_IMAGE_ASPECT_COLOR_BIT,
                      VK_ACCESS_TRANSFER_WRITE_BIT,
                      VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT,
                      VK_PIPELINE_STAGE_TRANSFER_BIT,
                      VK_PIPELINE_STAGE_ALL_COMMANDS_BIT);
        output_ahb_layout = VK_IMAGE_LAYOUT_GENERAL;
    } else {
        image_barrier(cmd, output_ahb_image, VK_IMAGE_LAYOUT_GENERAL,
                      VK_IMAGE_LAYOUT_GENERAL, VK_IMAGE_ASPECT_COLOR_BIT,
                      VK_ACCESS_SHADER_WRITE_BIT,
                      VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT,
                      VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT,
                      VK_PIPELINE_STAGE_ALL_COMMANDS_BIT);
    }

    if (s.frame_generation && s.synthetic_motion && s.fg_output_layout == VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL) {
        VkBufferImageCopy fg_copy = {};
        fg_copy.bufferOffset = 4;
        fg_copy.imageSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
        fg_copy.imageOffset = {(int32_t)(output_width / 2), (int32_t)(output_height / 2), 0};
        fg_copy.imageExtent = {1, 1, 1};
        vkCmdCopyImageToBuffer(cmd, s.fg_output_image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                               s.readback_buffer, 1, &fg_copy);
        image_barrier(cmd, s.fg_output_image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                      VK_IMAGE_LAYOUT_GENERAL, VK_IMAGE_ASPECT_COLOR_BIT,
                      VK_ACCESS_TRANSFER_READ_BIT,
                      VK_ACCESS_SHADER_READ_BIT | VK_ACCESS_SHADER_WRITE_BIT,
                      VK_PIPELINE_STAGE_TRANSFER_BIT,
                      VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT);
        s.fg_output_layout = VK_IMAGE_LAYOUT_GENERAL;
    }

    if (s.diagnostic_readback && !s.direct_output) {
        VkBufferImageCopy center_copy = {};
        center_copy.imageSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
        center_copy.imageOffset = {(int32_t)(output_width / 2), (int32_t)(output_height / 2), 0};
        center_copy.imageExtent = {1, 1, 1};
        vkCmdCopyImageToBuffer(cmd, s.output_image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                               s.readback_buffer, 1, &center_copy);
    }

    if (!s.direct_output) {
        image_barrier(cmd, s.output_image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                      VK_IMAGE_LAYOUT_GENERAL, VK_IMAGE_ASPECT_COLOR_BIT,
                      VK_ACCESS_TRANSFER_READ_BIT,
                      VK_ACCESS_SHADER_READ_BIT | VK_ACCESS_SHADER_WRITE_BIT,
                      VK_PIPELINE_STAGE_TRANSFER_BIT,
                      VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT);
        s.output_layout = VK_IMAGE_LAYOUT_GENERAL;
    }

    if (vkEndCommandBuffer(cmd) != VK_SUCCESS) return false;
    VkSubmitInfo submit = {VK_STRUCTURE_TYPE_SUBMIT_INFO};
    submit.commandBufferCount = 1;
    submit.pCommandBuffers = &cmd;
    if (trace) LOGI("submitting completed_frames=%llu", (unsigned long long)s.frame_id);
    if (vkQueueSubmit(s.vk.queue, 1, &submit, s.fence) != VK_SUCCESS) {
        LOGE("persistent FSR3 queue submit failed frame=%llu",
             (unsigned long long)s.frame_id);
        destroy_session();
        return false;
    }
    if (s.async_enabled) {
        s.fence_pending = true;
        s.last_submitted_output_index = output_index;
    } else if (vkWaitForFences(s.vk.device, 1, &s.fence, VK_TRUE, UINT64_MAX) !=
               VK_SUCCESS) {
        LOGE("persistent FSR3 GPU wait failed frame=%llu",
             (unsigned long long)s.frame_id);
        destroy_session();
        return false;
    }
    if (dispatch_timing_valid) {
        struct timespec dispatch_end;
        if (clock_gettime(CLOCK_MONOTONIC, &dispatch_end) == 0) {
            const double elapsed_ms =
                (dispatch_end.tv_sec - dispatch_start.tv_sec) * 1000.0 +
                (dispatch_end.tv_nsec - dispatch_start.tv_nsec) / 1000000.0;
            if (s.async_enabled) {
                g_async_submit_total_ms += elapsed_ms;
                g_async_submit_calls++;
                if (g_async_submit_calls == 1 || g_async_submit_calls % 120 == 0) {
                    LOGI("FSR async dispatch+submit timing: avg %.2fms over %llu frames",
                         g_async_submit_total_ms / g_async_submit_calls,
                         (unsigned long long)g_async_submit_calls);
                }
            } else {
                g_dispatch_timing_total_ms += elapsed_ms;
                g_dispatch_timing_calls++;
                if (g_dispatch_timing_calls == 1 ||
                    g_dispatch_timing_calls % 120 == 0) {
                    LOGI("FSR dispatch+GPU wait timing: avg %.2fms over %llu frames",
                         g_dispatch_timing_total_ms / g_dispatch_timing_calls,
                         (unsigned long long)g_dispatch_timing_calls);
                }
            }
        }
    }

    if (out_ready_output_ahb != nullptr && !s.async_enabled) {
        *out_ready_output_ahb = output_ahb;
    }
    if (trace) LOGI("queue wait complete completed_frames=%llu", (unsigned long long)s.frame_id);
    if (s.diagnostic_readback && !s.async_enabled) {
        memcpy(out_center_rgba, s.readback_mapped, 4);
    } else {
        memset(out_center_rgba, 0, 4);
    }
    uint8_t fg_center[4] = {};
    if (s.diagnostic_readback && s.frame_generation) {
        memcpy(fg_center, (uint8_t*)s.readback_mapped + 4, 4);
    }
    if (s.frame_id == 1 || s.frame_id % 120 == 0) {
        if (s.diagnostic_readback && s.frame_generation) {
            LOGI("persistent upscale+fg frame=%llu %ux%u -> %ux%u dt=%.2fms "
                 "depth=%d jitter=0 reset=1 motion=synthetic real=%u,%u,%u,%u "
                 "fg=%u,%u,%u,%u",
                 (unsigned long long)s.frame_id, input_width, input_height, output_width,
                 output_height, frame_time_ms,
                 (depth_data != nullptr || s.depth_ahb != nullptr) ? 1 : 0,
                 out_center_rgba[0], out_center_rgba[1], out_center_rgba[2],
                 out_center_rgba[3], fg_center[0], fg_center[1], fg_center[2], fg_center[3]);
        } else if (s.diagnostic_readback) {
            LOGI("persistent upscale frame=%llu %ux%u -> %ux%u dt=%.2fms "
                 "depth=%d jitter=0 reset=1 motion=%s centre=%u,%u,%u,%u",
                 (unsigned long long)s.frame_id, input_width, input_height, output_width,
                 output_height, frame_time_ms,
                 (depth_data != nullptr || s.depth_ahb != nullptr) ? 1 : 0,
                 s.synthetic_motion ? "synthetic" : "zero",
                 out_center_rgba[0], out_center_rgba[1], out_center_rgba[2],
                 out_center_rgba[3]);
        } else {
            LOGI("persistent upscale frame=%llu %ux%u -> %ux%u dt=%.2fms "
                 "depth=%d jitter=0 reset=1 motion=%s diagnostic_readback=off async=%d",
                 (unsigned long long)s.frame_id, input_width, input_height, output_width,
                 output_height, frame_time_ms,
                 (depth_data != nullptr || s.depth_ahb != nullptr) ? 1 : 0,
                 s.synthetic_motion ? "synthetic" : "zero",
                 s.async_enabled ? 1 : 0);
        }
    }
    return true;
}

extern "C" bool ds_fsr3_copy_fg_ahb(AHardwareBuffer* output_ahb) {
    UpscaleSession& s = g_session;
    if (!s.valid || !s.frame_generation || !s.synthetic_motion || s.fg_output_image == VK_NULL_HANDLE) {
        return false;
    }
    int output_index = ensure_output_ahb(output_ahb);
    if (output_index < 0) return false;
    VkImage output_ahb_image = s.output_ahb_images[output_index];
    VkImageLayout& output_ahb_layout = s.output_ahb_layouts[output_index];
    if (vkWaitForFences(s.vk.device, 1, &s.fence, VK_TRUE, UINT64_MAX) != VK_SUCCESS) {
        return false;
    }
    vkResetFences(s.vk.device, 1, &s.fence);
    if (vkResetCommandBuffer(s.command_buffer, 0) != VK_SUCCESS) return false;
    VkCommandBufferBeginInfo begin = {VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO};
    begin.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
    if (vkBeginCommandBuffer(s.command_buffer, &begin) != VK_SUCCESS) return false;
    VkCommandBuffer cmd = s.command_buffer;

    image_barrier(cmd, s.fg_output_image, s.fg_output_layout,
                  VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_ASPECT_COLOR_BIT,
                  VK_ACCESS_SHADER_WRITE_BIT, VK_ACCESS_TRANSFER_READ_BIT,
                  VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT);
    s.fg_output_layout = VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL;
    image_barrier(cmd, output_ahb_image, output_ahb_layout,
                  VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_IMAGE_ASPECT_COLOR_BIT,
                  0, VK_ACCESS_TRANSFER_WRITE_BIT,
                  VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT);
    VkImageCopy copy = {};
    copy.srcSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
    copy.dstSubresource = copy.srcSubresource;
    copy.extent = {s.output_width, s.output_height, 1};
    vkCmdCopyImage(cmd, s.fg_output_image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                   output_ahb_image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &copy);
    image_barrier(cmd, output_ahb_image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                  VK_IMAGE_LAYOUT_GENERAL, VK_IMAGE_ASPECT_COLOR_BIT,
                  VK_ACCESS_TRANSFER_WRITE_BIT,
                  VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT,
                  VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT);
    output_ahb_layout = VK_IMAGE_LAYOUT_GENERAL;
    image_barrier(cmd, s.fg_output_image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                  VK_IMAGE_LAYOUT_GENERAL, VK_IMAGE_ASPECT_COLOR_BIT,
                  VK_ACCESS_TRANSFER_READ_BIT,
                  VK_ACCESS_SHADER_READ_BIT | VK_ACCESS_SHADER_WRITE_BIT,
                  VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT);
    s.fg_output_layout = VK_IMAGE_LAYOUT_GENERAL;
    if (vkEndCommandBuffer(cmd) != VK_SUCCESS) return false;
    VkSubmitInfo submit = {VK_STRUCTURE_TYPE_SUBMIT_INFO};
    submit.commandBufferCount = 1;
    submit.pCommandBuffers = &cmd;
    if (vkQueueSubmit(s.vk.queue, 1, &submit, s.fence) != VK_SUCCESS ||
        vkWaitForFences(s.vk.device, 1, &s.fence, VK_TRUE, UINT64_MAX) != VK_SUCCESS) {
        LOGE("second FG AHB copy submit/wait failed");
        return false;
    }
    return true;
}
extern "C" void ds_fsr3_shutdown_ahb() {
    destroy_session();
}
