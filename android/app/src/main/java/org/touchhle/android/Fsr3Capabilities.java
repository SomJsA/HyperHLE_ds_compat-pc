package org.touchhle.android;

import android.content.Context;
import android.content.pm.FeatureInfo;
import android.content.pm.PackageManager;
import android.hardware.HardwareBuffer;
import android.os.Build;

import java.util.Arrays;

/** Non-invasive API-level capability probe for the future FSR3 Vulkan path. */
final class Fsr3Capabilities {
    private static final String FEATURE_VULKAN_VERSION = "android.hardware.vulkan.version";
    private static final String FEATURE_VULKAN_LEVEL = "android.hardware.vulkan.level";
    private static final String FEATURE_VULKAN_COMPUTE = "android.hardware.vulkan.compute";

    private Fsr3Capabilities() {}

    static boolean hasArm64Abi() {
        return Build.SUPPORTED_ABIS != null
                && Arrays.asList(Build.SUPPORTED_ABIS).contains("arm64-v8a");
    }

    static boolean supportsBasicVulkanPath(Context context) {
        PackageManager pm = context.getPackageManager();
        int vkVersion = featureVersion(pm, FEATURE_VULKAN_VERSION);
        boolean compute = pm.hasSystemFeature(FEATURE_VULKAN_COMPUTE);
        return hasArm64Abi()
                && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S
                && vkVersion >= 0x00402000
                && compute;
    }

    static String summary(Context context) {
        PackageManager pm = context.getPackageManager();
        int vkVersion = featureVersion(pm, FEATURE_VULKAN_VERSION);
        int vkLevel = featureVersion(pm, FEATURE_VULKAN_LEVEL);
        boolean compute = pm.hasSystemFeature(FEATURE_VULKAN_COMPUTE);

        String vulkan = vkVersion == 0
                ? "Vulkan 未知"
                : String.format("Vulkan %d.%d", (vkVersion >> 22) & 0x3ff,
                        (vkVersion >> 12) & 0x3ff);
        String level = vkLevel >= 0 ? " level " + vkLevel : "";
        String ahb = "AHardwareBuffer 探测需 Android 12+";
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            try {
                ahb = AhbProbe.isRgba8Supported()
                        ? "AHardwareBuffer RGBA8 可用"
                        : "AHardwareBuffer RGBA8 不可用";
            } catch (Throwable error) {
                // Keep the launcher usable on vendor runtimes that expose an
                // older framework surface despite reporting a newer API.
                ahb = "AHardwareBuffer 探测失败";
            }
        }

        boolean apiReady = Build.VERSION.SDK_INT >= Build.VERSION_CODES.S;
        boolean vkReady = vkVersion >= 0x00402000; // Vulkan 1.2
        boolean abiReady = hasArm64Abi();
        String gate = apiReady && vkReady && compute && abiReady
                ? "基础条件满足，仍需探测 Vulkan 队列与 GLES 外部内存"
                : "未满足 FSR3 前置条件";

        return "Android " + Build.VERSION.RELEASE + " (API " + Build.VERSION.SDK_INT + ")"
                + " · " + Build.SUPPORTED_ABIS[0] + " · " + vulkan + level
                + " · Compute " + (compute ? "可用" : "不可用")
                + " · " + ahb + "\n" + gate;
    }

    private static int featureVersion(PackageManager pm, String name) {
        FeatureInfo[] features = pm.getSystemAvailableFeatures();
        if (features == null) {
            return -1;
        }
        for (FeatureInfo feature : features) {
            if (feature != null && name.equals(feature.name)) {
                return Build.VERSION.SDK_INT >= Build.VERSION_CODES.N ? feature.version : -1;
            }
        }
        return -1;
    }

    /**
     * Keep HardwareBuffer method resolution out of the outer class. FSR3 only
     * loads this nested class on Android 12+, while older devices still launch
     * the native/FSR1 fallback without a NoClassDefFoundError or
     * NoSuchMethodError in the launcher summary.
     */
    private static final class AhbProbe {
        private AhbProbe() {}

        static boolean isRgba8Supported() {
            return HardwareBuffer.isSupported(
                    640, 960, HardwareBuffer.RGBA_8888, 1,
                    HardwareBuffer.USAGE_GPU_SAMPLED_IMAGE
                            | HardwareBuffer.USAGE_GPU_COLOR_OUTPUT);
        }
    }
}
