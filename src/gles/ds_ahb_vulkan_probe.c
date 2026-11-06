/*
 * Same-process Vulkan import probe for the AHardwareBuffer produced by the
 * Dead Space GLES bridge. This deliberately only proves import/bind; the FSR3
 * SDK integration will consume the resulting external image.
 */

#include <android/hardware_buffer.h>
#include <android/log.h>
#include <dlfcn.h>

#define VK_USE_PLATFORM_ANDROID_KHR
#include <vulkan/vulkan.h>

#include <stdbool.h>
#include <stdint.h>
#include <string.h>

#define LOG_TAG "DSFSR3"
#define LOGI(...) __android_log_print(ANDROID_LOG_INFO, LOG_TAG, __VA_ARGS__)
#define LOGE(...) __android_log_print(ANDROID_LOG_ERROR, LOG_TAG, __VA_ARGS__)

typedef struct VulkanApi {
    void *library;
    PFN_vkCreateInstance create_instance;
    PFN_vkEnumeratePhysicalDevices enumerate_physical_devices;
    PFN_vkGetPhysicalDeviceQueueFamilyProperties get_queue_family_properties;
    PFN_vkCreateDevice create_device;
    PFN_vkDestroyDevice destroy_device;
    PFN_vkDestroyInstance destroy_instance;
    PFN_vkCreateImage create_image;
    PFN_vkDestroyImage destroy_image;
    PFN_vkAllocateMemory allocate_memory;
    PFN_vkFreeMemory free_memory;
    PFN_vkBindImageMemory bind_image_memory;
    PFN_vkGetDeviceProcAddr get_device_proc_addr;
} VulkanApi;

static void *vulkan_symbol(VulkanApi *api, const char *name) {
    return dlsym(api->library, name);
}

static bool load_vulkan(VulkanApi *api) {
    memset(api, 0, sizeof(*api));
    api->library = dlopen("libvulkan.so", RTLD_NOW | RTLD_LOCAL);
    if (!api->library) {
        LOGE("Vulkan probe: dlopen(libvulkan.so) failed: %s", dlerror());
        return false;
    }
    api->create_instance = (PFN_vkCreateInstance)vulkan_symbol(api, "vkCreateInstance");
    api->enumerate_physical_devices = (PFN_vkEnumeratePhysicalDevices)vulkan_symbol(api, "vkEnumeratePhysicalDevices");
    api->get_queue_family_properties = (PFN_vkGetPhysicalDeviceQueueFamilyProperties)vulkan_symbol(api, "vkGetPhysicalDeviceQueueFamilyProperties");
    api->create_device = (PFN_vkCreateDevice)vulkan_symbol(api, "vkCreateDevice");
    api->destroy_device = (PFN_vkDestroyDevice)vulkan_symbol(api, "vkDestroyDevice");
    api->destroy_instance = (PFN_vkDestroyInstance)vulkan_symbol(api, "vkDestroyInstance");
    api->create_image = (PFN_vkCreateImage)vulkan_symbol(api, "vkCreateImage");
    api->destroy_image = (PFN_vkDestroyImage)vulkan_symbol(api, "vkDestroyImage");
    api->allocate_memory = (PFN_vkAllocateMemory)vulkan_symbol(api, "vkAllocateMemory");
    api->free_memory = (PFN_vkFreeMemory)vulkan_symbol(api, "vkFreeMemory");
    api->bind_image_memory = (PFN_vkBindImageMemory)vulkan_symbol(api, "vkBindImageMemory");
    api->get_device_proc_addr = (PFN_vkGetDeviceProcAddr)vulkan_symbol(api, "vkGetDeviceProcAddr");
    return api->create_instance && api->enumerate_physical_devices &&
           api->get_queue_family_properties && api->create_device &&
           api->destroy_device && api->destroy_instance && api->create_image &&
           api->destroy_image && api->allocate_memory && api->free_memory &&
           api->bind_image_memory && api->get_device_proc_addr;
}

bool ds_ahb_vulkan_probe(void *hardware_buffer) {
    if (!hardware_buffer) return false;
    AHardwareBuffer *ahb = (AHardwareBuffer *)hardware_buffer;
    VulkanApi api;
    if (!load_vulkan(&api)) return false;

    VkApplicationInfo app = {VK_STRUCTURE_TYPE_APPLICATION_INFO};
    app.pApplicationName = "deadspace-fsr3-ahb-probe";
    app.apiVersion = VK_API_VERSION_1_0;
    VkInstanceCreateInfo instance_info = {VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO};
    instance_info.pApplicationInfo = &app;
    VkInstance instance = VK_NULL_HANDLE;
    VkResult result = api.create_instance(&instance_info, NULL, &instance);
    if (result != VK_SUCCESS) {
        LOGE("Vulkan probe: vkCreateInstance=%d", result);
        return false;
    }

    uint32_t gpu_count = 0;
    result = api.enumerate_physical_devices(instance, &gpu_count, NULL);
    if (result != VK_SUCCESS || gpu_count == 0) {
        LOGE("Vulkan probe: no physical device (%d)", result);
        api.destroy_instance(instance, NULL);
        return false;
    }
    VkPhysicalDevice gpu = VK_NULL_HANDLE;
    api.enumerate_physical_devices(instance, &gpu_count, &gpu);

    uint32_t family_count = 0;
    api.get_queue_family_properties(gpu, &family_count, NULL);
    VkQueueFamilyProperties families[16] = {0};
    if (family_count > 16) family_count = 16;
    api.get_queue_family_properties(gpu, &family_count, families);
    uint32_t family = UINT32_MAX;
    for (uint32_t i = 0; i < family_count; ++i) {
        if (families[i].queueFlags & VK_QUEUE_GRAPHICS_BIT) {
            family = i;
            break;
        }
    }
    if (family == UINT32_MAX) {
        LOGE("Vulkan probe: no graphics queue family");
        api.destroy_instance(instance, NULL);
        return false;
    }

    const float priority = 1.0f;
    VkDeviceQueueCreateInfo queue_info = {VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO};
    queue_info.queueFamilyIndex = family;
    queue_info.queueCount = 1;
    queue_info.pQueuePriorities = &priority;
    const char *device_exts[] = {
        "VK_ANDROID_external_memory_android_hardware_buffer",
        VK_KHR_EXTERNAL_MEMORY_EXTENSION_NAME,
        VK_KHR_DEDICATED_ALLOCATION_EXTENSION_NAME,
    };
    VkDeviceCreateInfo device_info = {VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO};
    device_info.queueCreateInfoCount = 1;
    device_info.pQueueCreateInfos = &queue_info;
    device_info.enabledExtensionCount = (uint32_t)(sizeof(device_exts) / sizeof(device_exts[0]));
    device_info.ppEnabledExtensionNames = device_exts;
    VkDevice device = VK_NULL_HANDLE;
    result = api.create_device(gpu, &device_info, NULL, &device);
    if (result != VK_SUCCESS) {
        LOGE("Vulkan probe: vkCreateDevice=%d", result);
        api.destroy_instance(instance, NULL);
        return false;
    }

    PFN_vkGetAndroidHardwareBufferPropertiesANDROID get_props =
        (PFN_vkGetAndroidHardwareBufferPropertiesANDROID)api.get_device_proc_addr(
            device, "vkGetAndroidHardwareBufferPropertiesANDROID");
    if (!get_props) {
        LOGE("Vulkan probe: AHB properties entry point missing");
        api.destroy_device(device, NULL);
        api.destroy_instance(instance, NULL);
        return false;
    }

    VkAndroidHardwareBufferPropertiesANDROID props = {
        VK_STRUCTURE_TYPE_ANDROID_HARDWARE_BUFFER_PROPERTIES_ANDROID};
    VkAndroidHardwareBufferFormatPropertiesANDROID format_props = {
        VK_STRUCTURE_TYPE_ANDROID_HARDWARE_BUFFER_FORMAT_PROPERTIES_ANDROID};
    props.pNext = &format_props;
    result = get_props(device, ahb, &props);
    LOGI("Vulkan probe: getAHBProperties=%d size=%llu typeBits=0x%x format=%d",
         result, (unsigned long long)props.allocationSize, props.memoryTypeBits,
         format_props.format);
    if (result != VK_SUCCESS) {
        api.destroy_device(device, NULL);
        api.destroy_instance(instance, NULL);
        return false;
    }

    VkExternalMemoryImageCreateInfo external_info = {
        VK_STRUCTURE_TYPE_EXTERNAL_MEMORY_IMAGE_CREATE_INFO};
    external_info.handleTypes =
        VK_EXTERNAL_MEMORY_HANDLE_TYPE_ANDROID_HARDWARE_BUFFER_BIT_ANDROID;
    VkImageCreateInfo image_info = {VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO};
    image_info.pNext = &external_info;
    image_info.imageType = VK_IMAGE_TYPE_2D;
    image_info.format = format_props.format == VK_FORMAT_UNDEFINED
        ? VK_FORMAT_R8G8B8A8_UNORM : format_props.format;
    image_info.extent.width = 640;
    image_info.extent.height = 960;
    image_info.extent.depth = 1;
    image_info.mipLevels = 1;
    image_info.arrayLayers = 1;
    image_info.samples = VK_SAMPLE_COUNT_1_BIT;
    image_info.tiling = VK_IMAGE_TILING_OPTIMAL;
    image_info.usage = VK_IMAGE_USAGE_SAMPLED_BIT | VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT;
    image_info.sharingMode = VK_SHARING_MODE_EXCLUSIVE;
    image_info.initialLayout = VK_IMAGE_LAYOUT_UNDEFINED;
    VkImage image = VK_NULL_HANDLE;
    result = api.create_image(device, &image_info, NULL, &image);
    if (result != VK_SUCCESS) {
        LOGE("Vulkan probe: vkCreateImage=%d", result);
        api.destroy_device(device, NULL);
        api.destroy_instance(instance, NULL);
        return false;
    }

    VkMemoryDedicatedAllocateInfo dedicated = {
        VK_STRUCTURE_TYPE_MEMORY_DEDICATED_ALLOCATE_INFO};
    dedicated.image = image;
    VkImportAndroidHardwareBufferInfoANDROID import_info = {
        VK_STRUCTURE_TYPE_IMPORT_ANDROID_HARDWARE_BUFFER_INFO_ANDROID};
    import_info.buffer = ahb;
    import_info.pNext = &dedicated;
    VkMemoryAllocateInfo memory_info = {VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO};
    memory_info.pNext = &import_info;
    memory_info.allocationSize = props.allocationSize;
    memory_info.memoryTypeIndex = 0;
    for (uint32_t i = 0; i < 32; ++i) {
        if (props.memoryTypeBits & (1u << i)) {
            memory_info.memoryTypeIndex = i;
            break;
        }
    }
    VkDeviceMemory memory = VK_NULL_HANDLE;
    result = api.allocate_memory(device, &memory_info, NULL, &memory);
    if (result == VK_SUCCESS) {
        result = api.bind_image_memory(device, image, memory, 0);
    }
    LOGI("Vulkan probe: import=%d image=%p memory=%p type=%u",
         result, (void *)image, (void *)memory, memory_info.memoryTypeIndex);

    if (memory != VK_NULL_HANDLE) api.free_memory(device, memory, NULL);
    api.destroy_image(device, image, NULL);
    api.destroy_device(device, NULL);
    api.destroy_instance(instance, NULL);
    return result == VK_SUCCESS;
}
