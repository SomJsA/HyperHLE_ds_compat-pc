/*
 * Minimal GLES -> AHardwareBuffer bridge for the FSR3 integration path.
 *
 * This file deliberately stays independent of the FSR3 SDK. It owns one
 * AHardwareBuffer-backed GLES texture. The guest renderbuffer is copied into
 * that texture on the GL thread; a Vulkan bridge can later import the same
 * AHardwareBuffer with VK_ANDROID_external_memory_android_hardware_buffer.
 */

#include <android/hardware_buffer.h>
#include <android/log.h>
#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <GLES2/gl2.h>
#include <GLES2/gl2ext.h>
#include <GLES3/gl3.h>
#include <dlfcn.h>

#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define LOG_TAG "DSFSR3"
#define LOGI(...) __android_log_print(ANDROID_LOG_INFO, LOG_TAG, __VA_ARGS__)
#define LOGE(...) __android_log_print(ANDROID_LOG_ERROR, LOG_TAG, __VA_ARGS__)

typedef void (*PFN_glEGLImageTargetTexture2DOES)(GLenum, void *);
typedef void (*PFN_glEGLImageTargetRenderbufferStorageOES)(GLenum, void *);
typedef void (*PFN_ds_glBlitFramebuffer)(
    GLint, GLint, GLint, GLint, GLint, GLint, GLint, GLint, GLbitfield, GLenum);
typedef void (*PFN_glBindRenderbufferOES)(GLenum, GLuint);
typedef void (*PFN_glGetRenderbufferParameterivOES)(GLenum, GLenum, GLint *);
typedef EGLImageKHR (*PFN_ds_eglCreateImageKHR)(
    EGLDisplay, EGLContext, EGLenum, EGLClientBuffer, const EGLint *);
typedef EGLBoolean (*PFN_ds_eglDestroyImageKHR)(EGLDisplay, EGLImageKHR);

#define DS_EGL_GL_RENDERBUFFER_KHR 0x30B9
#define DS_GL_DEPTH_COMPONENT 0x1902
#define DS_GL_UNSIGNED_SHORT 0x1403
#define DS_GL_UNSIGNED_INT 0x1405
#define DS_GL_RENDERBUFFER_INTERNAL_FORMAT_OES 0x8D55
#define DS_GL_RENDERBUFFER_WIDTH_OES 0x8D42
#define DS_GL_RENDERBUFFER_HEIGHT_OES 0x8D43
#define DS_GL_RENDERBUFFER_BINDING_OES 0x8CA7

typedef int (*PFN_AHardwareBuffer_allocate)(const AHardwareBuffer_Desc *, AHardwareBuffer **);
typedef void (*PFN_AHardwareBuffer_release)(AHardwareBuffer *);
typedef void (*PFN_AHardwareBuffer_describe)(const AHardwareBuffer *, AHardwareBuffer_Desc *);
typedef int (*PFN_AHardwareBuffer_lock)(AHardwareBuffer *, uint64_t, int32_t, const ARect *, void **);
typedef int (*PFN_AHardwareBuffer_unlock)(AHardwareBuffer *, int32_t *);

typedef struct AhbApi {
    void *library;
    PFN_AHardwareBuffer_allocate allocate;
    PFN_AHardwareBuffer_release release;
    PFN_AHardwareBuffer_describe describe;
    PFN_AHardwareBuffer_lock lock;
    PFN_AHardwareBuffer_unlock unlock;
} AhbApi;

static AhbApi *ahb_api(void) {
    static AhbApi api;
    if (api.library == NULL) {
        api.library = dlopen("libandroid.so", RTLD_NOW | RTLD_LOCAL);
        if (api.library != NULL) {
            api.allocate = (PFN_AHardwareBuffer_allocate)dlsym(api.library, "AHardwareBuffer_allocate");
            api.release = (PFN_AHardwareBuffer_release)dlsym(api.library, "AHardwareBuffer_release");
            api.describe = (PFN_AHardwareBuffer_describe)dlsym(api.library, "AHardwareBuffer_describe");
            api.lock = (PFN_AHardwareBuffer_lock)dlsym(api.library, "AHardwareBuffer_lock");
            api.unlock = (PFN_AHardwareBuffer_unlock)dlsym(api.library, "AHardwareBuffer_unlock");
        }
    }
    if (api.allocate == NULL || api.release == NULL || api.describe == NULL ||
        api.lock == NULL || api.unlock == NULL) {
        return NULL;
    }
    return &api;
}

static AHardwareBuffer *g_buffer;
static EGLImageKHR g_image;
static GLuint g_texture;
#define DS_OUTPUT_BUFFER_COUNT 2
static AHardwareBuffer *g_output_buffers[DS_OUTPUT_BUFFER_COUNT];
static EGLImageKHR g_output_images[DS_OUTPUT_BUFFER_COUNT] = {EGL_NO_IMAGE_KHR, EGL_NO_IMAGE_KHR};
static GLuint g_output_textures[DS_OUTPUT_BUFFER_COUNT];
static int g_output_write_index = -1;
static GLuint g_source_fbo;
static int g_width;
static int g_height;
static int g_output_width;
static int g_output_height;
static bool g_initialized;

typedef bool (*PFN_ds_fsr3_upscale_ahb)(
    AHardwareBuffer *, AHardwareBuffer *, AHardwareBuffer *, uint32_t, uint32_t,
    uint32_t, uint32_t, const float *, float, uint8_t *, AHardwareBuffer **);
typedef bool (*PFN_ds_fsr3_copy_fg_ahb)(AHardwareBuffer *);
typedef AHardwareBuffer *(*PFN_ds_fsr3_wait_pending_ahb)(void);
typedef void (*PFN_ds_fsr3_shutdown_ahb)(void);
static void *g_fsr3_library;
static PFN_ds_fsr3_upscale_ahb g_fsr3_upscale;
static PFN_ds_fsr3_copy_fg_ahb g_fsr3_copy_fg;
static PFN_ds_fsr3_wait_pending_ahb g_fsr3_wait_pending;
static uint64_t g_fsr3_upscale_calls;
static GLuint g_ready_output_texture;
static int g_ready_output_index = -1;
static AHardwareBuffer *g_ready_output_buffer;
static uint64_t g_copy_timing_calls;
static double g_copy_timing_total_ms;
static uint64_t g_depth_image_calls;
static uint64_t g_depth_read_reuse_calls;
static uint64_t g_depth_read_failures;
static uint64_t g_depth_blit_failures;
static bool g_depth_image_failure_logged;
static EGLImageKHR g_cached_depth_image = EGL_NO_IMAGE_KHR;
static EGLDisplay g_cached_depth_display = EGL_NO_DISPLAY;
static EGLContext g_cached_depth_context = EGL_NO_CONTEXT;
static GLuint g_cached_depth_renderbuffer;
static int g_cached_depth_width;
static int g_cached_depth_height;
static EGLContext g_depth_read_context = EGL_NO_CONTEXT;
static EGLImageKHR g_depth_read_image = EGL_NO_IMAGE_KHR;
static GLuint g_depth_read_fbo;
static GLuint g_depth_read_renderbuffer;
static int g_depth_read_width;
static int g_depth_read_height;
static uint16_t *g_depth_raw_buffer;
static size_t g_depth_raw_capacity;
static AHardwareBuffer *g_depth_ahb_buffer;
static EGLImageKHR g_depth_ahb_image = EGL_NO_IMAGE_KHR;
static EGLContext g_depth_ahb_context = EGL_NO_CONTEXT;
static GLuint g_depth_ahb_texture;
static GLuint g_depth_ahb_fbo;
static int g_depth_ahb_width;
static int g_depth_ahb_height;
static struct timespec g_last_fsr3_call;
static bool g_have_last_fsr3_call;
static float g_last_frame_time_ms = 16.67f;

uint32_t ds_ahb_bridge_output_texture(void);
void ds_ahb_bridge_destroy(void);



void *ds_ahb_bridge_handle(void) {
    return g_buffer;
}

static bool fsr3_async_requested(void) {
    const char *value = getenv("TOUCHHLE_DS_FSR3_ASYNC");
    return value != NULL && strcmp(value, "1") == 0;
}

static bool load_fsr3_library(void) {
    if (!g_fsr3_upscale) {
        g_fsr3_library = dlopen("libdsfsr3bridge.so", RTLD_NOW | RTLD_LOCAL);
        if (!g_fsr3_library) {
            LOGE("dlopen(libdsfsr3bridge.so) failed: %s", dlerror());
            return false;
        }
        g_fsr3_upscale =
            (PFN_ds_fsr3_upscale_ahb)dlsym(g_fsr3_library, "ds_fsr3_upscale_ahb");
        g_fsr3_copy_fg =
            (PFN_ds_fsr3_copy_fg_ahb)dlsym(g_fsr3_library, "ds_fsr3_copy_fg_ahb");
        g_fsr3_wait_pending =
            (PFN_ds_fsr3_wait_pending_ahb)dlsym(
                g_fsr3_library, "ds_fsr3_wait_pending_ahb");
        if (!g_fsr3_upscale) {
            LOGE("ds_fsr3_upscale_ahb symbol missing");
            dlclose(g_fsr3_library);
            g_fsr3_library = NULL;
            return false;
        }
    }
    return true;
}

uint32_t ds_ahb_bridge_fsr3_wait_pending(void) {
    if (!g_initialized || !fsr3_async_requested() || !load_fsr3_library() ||
        g_fsr3_wait_pending == NULL) {
        return 0;
    }
    AHardwareBuffer *ready = g_fsr3_wait_pending();
    if (ready == NULL) return 0;

    int ready_index = -1;
    for (int i = 0; i < DS_OUTPUT_BUFFER_COUNT; ++i) {
        if (g_output_buffers[i] == ready) {
            ready_index = i;
            break;
        }
    }
    if (ready_index < 0 || g_output_textures[ready_index] == 0) {
        LOGE("async FSR ready output AHB is not a cached GL output");
        return 0;
    }
    g_ready_output_index = ready_index;
    g_ready_output_buffer = ready;
    g_ready_output_texture = g_output_textures[ready_index];
    if (g_fsr3_upscale_calls == 0 || g_fsr3_upscale_calls % 120 == 0) {
        LOGI("async FSR previous output ready index=%d texture=%u buffer=%p",
             ready_index, g_ready_output_texture, (void *)ready);
    }
    return g_ready_output_texture;
}

bool ds_ahb_bridge_fsr3_upscale(
    const float *depth_data,
    void *depth_ahb,
    uint8_t out_rgba[4]
) {
    if (!g_initialized || out_rgba == NULL || !load_fsr3_library()) {
        return false;
    }
    g_fsr3_upscale_calls++;
    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now) == 0) {
        if (g_have_last_fsr3_call) {
            const double elapsed_ms =
                (now.tv_sec - g_last_fsr3_call.tv_sec) * 1000.0 +
                (now.tv_nsec - g_last_fsr3_call.tv_nsec) / 1000000.0;
            if (elapsed_ms >= 1.0 && elapsed_ms <= 100.0) {
                g_last_frame_time_ms = (float)elapsed_ms;
            }
        }
        g_last_fsr3_call = now;
        g_have_last_fsr3_call = true;
    }
    int next_index = (g_output_write_index + 1) % DS_OUTPUT_BUFFER_COUNT;
    AHardwareBuffer *ready_output = NULL;
    bool ok = g_fsr3_upscale(
        g_buffer, g_output_buffers[next_index], (AHardwareBuffer *)depth_ahb,
        (uint32_t)g_width, (uint32_t)g_height, (uint32_t)g_output_width,
        (uint32_t)g_output_height, depth_data, g_last_frame_time_ms, out_rgba,
        &ready_output);
    if (ok) {
        g_output_write_index = next_index;
    }
    if (g_fsr3_upscale_calls == 1 || g_fsr3_upscale_calls % 120 == 0) {
        LOGI("FSR3 AHB upscale call=%llu result=%d frameTime=%.2fms centre=%u,%u,%u,%u outputIndex=%d outputTexture=%u async=%d ready=%p",
             (unsigned long long)g_fsr3_upscale_calls, ok, g_last_frame_time_ms,
             out_rgba[0], out_rgba[1], out_rgba[2], out_rgba[3],
             g_output_write_index,
             ds_ahb_bridge_output_texture(), fsr3_async_requested() ? 1 : 0,
             (void *)ready_output);
    }
    return ok;
}

uint32_t ds_ahb_bridge_output_texture(void) {
    if (g_output_write_index < 0 || g_output_write_index >= DS_OUTPUT_BUFFER_COUNT) {
        return 0;
    }
    return g_output_textures[g_output_write_index];
}

void ds_ahb_bridge_output_size(int *width, int *height) {
    if (width != NULL) *width = g_output_width;
    if (height != NULL) *height = g_output_height;
}

static bool extension_list_contains(const char *list, const char *name) {
    if (list == NULL || name == NULL || *name == '\0') return false;
    const size_t name_length = strlen(name);
    const char *cursor = list;
    while ((cursor = strstr(cursor, name)) != NULL) {
        const char before = cursor == list ? ' ' : cursor[-1];
        const char after = cursor[name_length];
        if ((before == ' ' || before == '\0') &&
            (after == ' ' || after == '\0')) {
            return true;
        }
        cursor += name_length;
    }
    return false;
}

/*
 * EGL_KHR_gl_renderbuffer_image lets a shared ES2 context observe a guest
 * renderbuffer without assuming that renderbuffer names are shared. EGL share
 * groups only share texture/buffer names; renderbuffers are private.
 */
void *ds_depth_image_create_from_renderbuffer(GLuint depth_rb, int width, int height) {
    if (depth_rb == 0 || width <= 0 || height <= 0) return NULL;
    EGLDisplay display = eglGetCurrentDisplay();
    EGLContext context = eglGetCurrentContext();
    if (display == EGL_NO_DISPLAY || context == EGL_NO_CONTEXT) {
        LOGE("depth EGLImage: no current EGL display/context");
        return NULL;
    }
    if (g_cached_depth_image != EGL_NO_IMAGE_KHR &&
        g_cached_depth_context == context &&
        g_cached_depth_renderbuffer == depth_rb &&
        g_cached_depth_width == width &&
        g_cached_depth_height == height) {
        g_depth_image_calls++;
        if (g_depth_image_calls == 1 || g_depth_image_calls % 120 == 0) {
            LOGI("depth EGLImage cache hit: call=%llu rb=%u size=%dx%d image=%p",
                 (unsigned long long)g_depth_image_calls, depth_rb, width, height,
                 (void *)g_cached_depth_image);
        }
        return (void *)g_cached_depth_image;
    }

    PFN_ds_eglDestroyImageKHR destroy_image =
        (PFN_ds_eglDestroyImageKHR)eglGetProcAddress("eglDestroyImageKHR");
    if (g_cached_depth_image != EGL_NO_IMAGE_KHR) {
        if (g_cached_depth_display != EGL_NO_DISPLAY && destroy_image != NULL) {
            destroy_image(g_cached_depth_display, g_cached_depth_image);
        }
        g_cached_depth_image = EGL_NO_IMAGE_KHR;
        g_cached_depth_display = EGL_NO_DISPLAY;
        g_cached_depth_context = EGL_NO_CONTEXT;
        g_cached_depth_renderbuffer = 0;
        g_cached_depth_width = 0;
        g_cached_depth_height = 0;
    }
    if (!extension_list_contains(eglQueryString(display, EGL_EXTENSIONS),
                                 "EGL_KHR_gl_renderbuffer_image")) {
        if (!g_depth_image_failure_logged) {
            g_depth_image_failure_logged = true;
            LOGE("depth EGLImage unavailable: EGL_KHR_gl_renderbuffer_image missing");
        }
        return NULL;
    }
    if (!extension_list_contains((const char *)glGetString(GL_EXTENSIONS),
                                 "GL_OES_EGL_image")) {
        if (!g_depth_image_failure_logged) {
            g_depth_image_failure_logged = true;
            LOGE("depth EGLImage unavailable: GL_OES_EGL_image missing");
        }
        return NULL;
    }

    PFN_ds_eglCreateImageKHR create_image =
        (PFN_ds_eglCreateImageKHR)eglGetProcAddress("eglCreateImageKHR");
    if (create_image == NULL) {
        if (!g_depth_image_failure_logged) {
            g_depth_image_failure_logged = true;
            LOGE("depth EGLImage unavailable: eglCreateImageKHR missing");
        }
        return NULL;
    }

    GLint internal_format = 0;
    GLint actual_width = 0;
    GLint actual_height = 0;
    PFN_glBindRenderbufferOES bind_renderbuffer =
        (PFN_glBindRenderbufferOES)eglGetProcAddress("glBindRenderbufferOES");
    PFN_glGetRenderbufferParameterivOES get_parameter =
        (PFN_glGetRenderbufferParameterivOES)eglGetProcAddress(
            "glGetRenderbufferParameterivOES");
    if (bind_renderbuffer != NULL && get_parameter != NULL) {
        GLint previous_rb = 0;
        glGetIntegerv(DS_GL_RENDERBUFFER_BINDING_OES, &previous_rb);
        bind_renderbuffer(GL_RENDERBUFFER, depth_rb);
        get_parameter(GL_RENDERBUFFER, DS_GL_RENDERBUFFER_INTERNAL_FORMAT_OES,
                      &internal_format);
        get_parameter(GL_RENDERBUFFER, DS_GL_RENDERBUFFER_WIDTH_OES, &actual_width);
        get_parameter(GL_RENDERBUFFER, DS_GL_RENDERBUFFER_HEIGHT_OES, &actual_height);
        bind_renderbuffer(GL_RENDERBUFFER, (GLuint)previous_rb);
    }

    EGLImageKHR image = create_image(
        display, context, DS_EGL_GL_RENDERBUFFER_KHR,
        (EGLClientBuffer)(uintptr_t)depth_rb, NULL);
    if (image == EGL_NO_IMAGE_KHR) {
        if (!g_depth_image_failure_logged) {
            g_depth_image_failure_logged = true;
            LOGE("eglCreateImageKHR(EGL_GL_RENDERBUFFER_KHR) failed: 0x%x "
                 "rb=%u requested=%dx%d actual=%dx%d internal=0x%x",
                 eglGetError(), depth_rb, width, height, actual_width, actual_height,
                 internal_format);
        }
        return NULL;
    }
    g_depth_image_calls++;
    if (g_depth_image_calls == 1 || g_depth_image_calls % 120 == 0) {
        LOGI("depth EGLImage ready: call=%llu rb=%u requested=%dx%d actual=%dx%d "
             "internal=0x%x image=%p",
             (unsigned long long)g_depth_image_calls, depth_rb, width, height,
             actual_width, actual_height, internal_format, (void *)image);
    }
    g_cached_depth_image = image;
    g_cached_depth_display = display;
    g_cached_depth_context = context;
    g_cached_depth_renderbuffer = depth_rb;
    g_cached_depth_width = width;
    g_cached_depth_height = height;
    return (void *)image;
}

static GLuint compile_depth_shader(GLenum type, const char *source) {
    GLuint shader = glCreateShader(type);
    if (shader == 0) return 0;
    glShaderSource(shader, 1, &source, NULL);
    glCompileShader(shader);
    GLint compiled = 0;
    glGetShaderiv(shader, GL_COMPILE_STATUS, &compiled);
    if (!compiled) {
        GLchar log[512] = {0};
        glGetShaderInfoLog(shader, sizeof(log), NULL, log);
        LOGE("depth blit shader compile failed: %s", log);
        glDeleteShader(shader);
        return 0;
    }
    return shader;
}

void ds_depth_ahb_probe(int width, int height) {
    static bool probed;
    if (probed || width <= 0 || height <= 0) return;
    probed = true;

    AhbApi *api = ahb_api();
    if (api == NULL) {
        LOGE("depth AHB probe unavailable: AHardwareBuffer API missing");
        return;
    }
    AHardwareBuffer_Desc desc = {
        .width = (uint32_t)width,
        .height = (uint32_t)height,
        .layers = 1,
        .format = AHARDWAREBUFFER_FORMAT_D16_UNORM,
        .usage = AHARDWAREBUFFER_USAGE_GPU_SAMPLED_IMAGE |
                 AHARDWAREBUFFER_USAGE_GPU_FRAMEBUFFER,
    };
    AHardwareBuffer *buffer = NULL;
    int allocate_result = api->allocate(&desc, &buffer);
    if (allocate_result != 0 || buffer == NULL) {
        LOGE("depth AHB probe allocate failed: result=%d size=%dx%d",
             allocate_result, width, height);
        return;
    }

    EGLDisplay display = eglGetCurrentDisplay();
    PFNEGLGETNATIVECLIENTBUFFERANDROIDPROC get_native =
        (PFNEGLGETNATIVECLIENTBUFFERANDROIDPROC)eglGetProcAddress(
            "eglGetNativeClientBufferANDROID");
    PFNEGLCREATEIMAGEKHRPROC create_image =
        (PFNEGLCREATEIMAGEKHRPROC)eglGetProcAddress("eglCreateImageKHR");
    PFNEGLDESTROYIMAGEKHRPROC destroy_image =
        (PFNEGLDESTROYIMAGEKHRPROC)eglGetProcAddress("eglDestroyImageKHR");
    PFN_glEGLImageTargetTexture2DOES image_target =
        (PFN_glEGLImageTargetTexture2DOES)eglGetProcAddress(
            "glEGLImageTargetTexture2DOES");
    if (display == EGL_NO_DISPLAY || get_native == NULL || create_image == NULL ||
        image_target == NULL) {
        LOGE("depth AHB probe unavailable: EGL import entry points missing");
        api->release(buffer);
        return;
    }

    EGLClientBuffer client = get_native(buffer);
    if (client == NULL) {
        LOGE("depth AHB probe native client buffer failed: 0x%x", eglGetError());
        api->release(buffer);
        return;
    }
    const EGLint image_attribs[] = {EGL_NONE};
    EGLImageKHR image = create_image(
        display, EGL_NO_CONTEXT, EGL_NATIVE_BUFFER_ANDROID, client, image_attribs);
    if (image == EGL_NO_IMAGE_KHR) {
        LOGE("depth AHB probe EGLImage failed: client=%p error=0x%x",
             (void *)client, eglGetError());
        api->release(buffer);
        return;
    }

    GLuint texture = 0;
    GLuint fbo = 0;
    glGenTextures(1, &texture);
    glBindTexture(GL_TEXTURE_2D, texture);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
    image_target(GL_TEXTURE_2D, image);
    GLenum import_error = glGetError();

    if (import_error == GL_NO_ERROR) {
        glGenFramebuffers(1, &fbo);
        glBindFramebuffer(GL_FRAMEBUFFER, fbo);
        glFramebufferTexture2D(
            GL_FRAMEBUFFER, GL_DEPTH_ATTACHMENT, GL_TEXTURE_2D, texture, 0);
        GLenum no_color = GL_NONE;
        glDrawBuffers(1, &no_color);
        glReadBuffer(GL_NONE);
    }
    GLenum fbo_status = import_error == GL_NO_ERROR
        ? glCheckFramebufferStatus(GL_FRAMEBUFFER)
        : 0;
    LOGI("depth AHB probe: allocate=%d format=0x%x image=%p importError=0x%x "
         "fbo=0x%x",
         allocate_result, AHARDWAREBUFFER_FORMAT_D16_UNORM, (void *)image,
         import_error, fbo_status);

    glBindFramebuffer(GL_FRAMEBUFFER, 0);
    if (fbo != 0) glDeleteFramebuffers(1, &fbo);
    if (texture != 0) glDeleteTextures(1, &texture);
    if (destroy_image != NULL) destroy_image(display, image);
    api->release(buffer);
}

static bool ensure_depth_ahb_target(int width, int height) {
    EGLContext context = eglGetCurrentContext();
    if (g_depth_ahb_buffer != NULL &&
        g_depth_ahb_context == context &&
        g_depth_ahb_width == width &&
        g_depth_ahb_height == height &&
        g_depth_ahb_texture != 0 &&
        g_depth_ahb_fbo != 0) {
        return true;
    }

    EGLDisplay display = eglGetCurrentDisplay();
    PFN_ds_eglDestroyImageKHR destroy_image =
        (PFN_ds_eglDestroyImageKHR)eglGetProcAddress("eglDestroyImageKHR");
    if (g_depth_ahb_context == context) {
        if (g_depth_ahb_fbo != 0) {
            glDeleteFramebuffers(1, &g_depth_ahb_fbo);
        }
        if (g_depth_ahb_texture != 0) {
            glDeleteTextures(1, &g_depth_ahb_texture);
        }
    }
    g_depth_ahb_fbo = 0;
    g_depth_ahb_texture = 0;
    if (g_depth_ahb_image != EGL_NO_IMAGE_KHR) {
        if (g_depth_ahb_context == context && display != EGL_NO_DISPLAY &&
            destroy_image != NULL) {
            destroy_image(display, g_depth_ahb_image);
        }
        g_depth_ahb_image = EGL_NO_IMAGE_KHR;
    }
    g_depth_ahb_context = EGL_NO_CONTEXT;
    if (g_depth_ahb_buffer != NULL &&
        (g_depth_ahb_width != width || g_depth_ahb_height != height)) {
        AhbApi *api = ahb_api();
        if (api != NULL) api->release(g_depth_ahb_buffer);
        g_depth_ahb_buffer = NULL;
    }

    AhbApi *api = ahb_api();
    if (api == NULL) return false;
    if (g_depth_ahb_buffer == NULL) {
        AHardwareBuffer_Desc desc = {
            .width = (uint32_t)width,
            .height = (uint32_t)height,
            .layers = 1,
            .format = AHARDWAREBUFFER_FORMAT_D16_UNORM,
            .usage = AHARDWAREBUFFER_USAGE_GPU_SAMPLED_IMAGE |
                     AHARDWAREBUFFER_USAGE_GPU_FRAMEBUFFER,
        };
        if (api->allocate(&desc, &g_depth_ahb_buffer) != 0 ||
            g_depth_ahb_buffer == NULL) {
            g_depth_ahb_buffer = NULL;
            return false;
        }
    }

    PFNEGLGETNATIVECLIENTBUFFERANDROIDPROC get_native =
        (PFNEGLGETNATIVECLIENTBUFFERANDROIDPROC)eglGetProcAddress(
            "eglGetNativeClientBufferANDROID");
    PFNEGLCREATEIMAGEKHRPROC create_image =
        (PFNEGLCREATEIMAGEKHRPROC)eglGetProcAddress("eglCreateImageKHR");
    PFN_glEGLImageTargetTexture2DOES image_target =
        (PFN_glEGLImageTargetTexture2DOES)eglGetProcAddress(
            "glEGLImageTargetTexture2DOES");
    if (display == EGL_NO_DISPLAY || context == EGL_NO_CONTEXT ||
        get_native == NULL || create_image == NULL || image_target == NULL) {
        return false;
    }
    EGLClientBuffer client = get_native(g_depth_ahb_buffer);
    if (client == NULL) return false;
    const EGLint image_attribs[] = {EGL_NONE};
    g_depth_ahb_image = create_image(
        display, EGL_NO_CONTEXT, EGL_NATIVE_BUFFER_ANDROID, client, image_attribs);
    if (g_depth_ahb_image == EGL_NO_IMAGE_KHR) return false;

    GLint previous_texture = 0;
    glGetIntegerv(GL_TEXTURE_BINDING_2D, &previous_texture);
    glGenTextures(1, &g_depth_ahb_texture);
    glBindTexture(GL_TEXTURE_2D, g_depth_ahb_texture);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
    image_target(GL_TEXTURE_2D, g_depth_ahb_image);
    glBindTexture(GL_TEXTURE_2D, (GLuint)previous_texture);
    if (glGetError() != GL_NO_ERROR) return false;

    glGenFramebuffers(1, &g_depth_ahb_fbo);
    glBindFramebuffer(GL_FRAMEBUFFER, g_depth_ahb_fbo);
    glFramebufferTexture2D(
        GL_FRAMEBUFFER, GL_DEPTH_ATTACHMENT, GL_TEXTURE_2D, g_depth_ahb_texture, 0);
    {
        GLenum no_color = GL_NONE;
        glDrawBuffers(1, &no_color);
        glReadBuffer(GL_NONE);
    }
    if (glCheckFramebufferStatus(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE) {
        return false;
    }
    g_depth_ahb_context = context;
    g_depth_ahb_width = width;
    g_depth_ahb_height = height;
    LOGI("depth AHB target ready: %dx%d buffer=%p texture=%u fbo=%u",
         width, height, (void *)g_depth_ahb_buffer,
         g_depth_ahb_texture, g_depth_ahb_fbo);
    return true;
}

bool ds_depth_image_copy_to_ahb(void *opaque_image, int width, int height) {
    if (opaque_image == NULL || width <= 0 || height <= 0) return false;
    EGLImageKHR image = (EGLImageKHR)opaque_image;
    PFN_glEGLImageTargetRenderbufferStorageOES image_target =
        (PFN_glEGLImageTargetRenderbufferStorageOES)eglGetProcAddress(
            "glEGLImageTargetRenderbufferStorageOES");
    if (image_target == NULL || !ensure_depth_ahb_target(width, height)) return false;

    EGLContext current_context = eglGetCurrentContext();
    PFN_ds_glBlitFramebuffer blit_framebuffer =
        (PFN_ds_glBlitFramebuffer)eglGetProcAddress("glBlitFramebufferEXT");
    if (blit_framebuffer == NULL) {
        blit_framebuffer = (PFN_ds_glBlitFramebuffer)eglGetProcAddress(
            "glBlitFramebuffer");
    }
    if (blit_framebuffer == NULL) {
        g_depth_blit_failures++;
        if (g_depth_blit_failures == 1 || g_depth_blit_failures % 120 == 0) {
            LOGE("depth AHB blit entry point missing");
        }
        return false;
    }
    bool reuse_source = g_depth_read_context == current_context &&
                        g_depth_read_image == image &&
                        g_depth_read_fbo != 0 &&
                        g_depth_read_renderbuffer != 0 &&
                        g_depth_read_width == width &&
                        g_depth_read_height == height;
    GLuint source_fbo = g_depth_read_fbo;
    GLuint source_rb = g_depth_read_renderbuffer;
    GLint previous_fbo = 0;
    glGetIntegerv(GL_FRAMEBUFFER_BINDING, &previous_fbo);
    if (!reuse_source) {
        if (g_depth_read_context == current_context) {
            if (g_depth_read_fbo != 0) glDeleteFramebuffers(1, &g_depth_read_fbo);
            if (g_depth_read_renderbuffer != 0) {
                glDeleteRenderbuffers(1, &g_depth_read_renderbuffer);
            }
        }
        glGenFramebuffers(1, &source_fbo);
        glGenRenderbuffers(1, &source_rb);
        glBindRenderbuffer(GL_RENDERBUFFER, source_rb);
        image_target(GL_RENDERBUFFER, image);
        if (glGetError() != GL_NO_ERROR) {
            glDeleteRenderbuffers(1, &source_rb);
            glDeleteFramebuffers(1, &source_fbo);
            glBindFramebuffer(GL_FRAMEBUFFER, (GLuint)previous_fbo);
            return false;
        }
        glBindFramebuffer(GL_FRAMEBUFFER, source_fbo);
        glFramebufferRenderbuffer(
            GL_FRAMEBUFFER, GL_DEPTH_ATTACHMENT, GL_RENDERBUFFER, source_rb);
        {
            GLenum no_color = GL_NONE;
            glDrawBuffers(1, &no_color);
            glReadBuffer(GL_NONE);
        }
        if (glCheckFramebufferStatus(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE) {
            glDeleteRenderbuffers(1, &source_rb);
            glDeleteFramebuffers(1, &source_fbo);
            glBindFramebuffer(GL_FRAMEBUFFER, (GLuint)previous_fbo);
            return false;
        }
        g_depth_read_context = current_context;
        g_depth_read_image = image;
        g_depth_read_fbo = source_fbo;
        g_depth_read_renderbuffer = source_rb;
        g_depth_read_width = width;
        g_depth_read_height = height;
    }

    glBindFramebuffer(GL_READ_FRAMEBUFFER, source_fbo);
    glReadBuffer(GL_NONE);
    glBindFramebuffer(GL_DRAW_FRAMEBUFFER, g_depth_ahb_fbo);
    {
        GLenum no_color = GL_NONE;
        glDrawBuffers(1, &no_color);
    }
    blit_framebuffer(0, 0, width, height, 0, 0, width, height,
                     GL_DEPTH_BUFFER_BIT, GL_NEAREST);
    GLenum blit_error = glGetError();
    glBindFramebuffer(GL_FRAMEBUFFER, (GLuint)previous_fbo);
    if (blit_error != GL_NO_ERROR) {
        g_depth_blit_failures++;
        if (g_depth_blit_failures == 1 || g_depth_blit_failures % 120 == 0) {
            LOGE("depth AHB blit failed: 0x%x count=%llu",
                 blit_error, (unsigned long long)g_depth_blit_failures);
        }
        return false;
    }
    return true;
}

void *ds_depth_ahb_handle(void) {
    return g_depth_ahb_buffer;
}

bool ds_depth_image_read(void *opaque_image, int width, int height, float *depth) {
    if (opaque_image == NULL || width <= 0 || height <= 0 || depth == NULL) return false;
    EGLImageKHR image = (EGLImageKHR)opaque_image;
    PFN_glEGLImageTargetRenderbufferStorageOES image_target =
        (PFN_glEGLImageTargetRenderbufferStorageOES)eglGetProcAddress(
            "glEGLImageTargetRenderbufferStorageOES");
    if (image_target == NULL) {
        LOGE("depth EGLImage read unavailable: target renderbuffer entry point missing");
        return false;
    }

    static const char *vertex_source =
        "#version 300 es\n"
        "out vec2 vUv;\n"
        "void main() {\n"
        "  vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));\n"
        "  vUv = p;\n"
        "  gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);\n"
        "}\n";
    static const char *fragment_source =
        "#version 300 es\n"
        "precision highp float;\n"
        "precision highp int;\n"
        "uniform sampler2D uDepth;\n"
        "in vec2 vUv;\n"
        "out vec4 outColor;\n"
        "void main() {\n"
        "  uint bits = floatBitsToUint(texture(uDepth, vUv).r);\n"
        "  outColor = vec4(float((bits >> 24) & 255u), float((bits >> 16) & 255u),\n"
        "                  float((bits >> 8) & 255u), float(bits & 255u)) / 255.0;\n"
        "}\n";

    const size_t pixel_count = (size_t)width * (size_t)height;
    GLuint source_fbo = 0;
    GLuint image_rb = 0;
    GLuint blit_fbo = 0;
    GLuint color_fbo = 0;
    GLuint depth_texture = 0;
    GLuint color_texture = 0;
    GLuint vao = 0;
    GLuint vertex_shader = 0;
    GLuint fragment_shader = 0;
    GLuint program = 0;
    uint8_t *packed = NULL;
    bool read_ok = false;
    GLenum read_error = GL_NO_ERROR;
    GLint previous_fbo = 0;
    GLint previous_viewport[4] = {0, 0, 0, 0};

    glGetIntegerv(GL_FRAMEBUFFER_BINDING, &previous_fbo);
    glGetIntegerv(GL_VIEWPORT, previous_viewport);
    while (glGetError() != GL_NO_ERROR) {}

    EGLContext current_context = eglGetCurrentContext();
    bool reuse_source = g_depth_read_context == current_context &&
                        g_depth_read_image == image &&
                        g_depth_read_fbo != 0 &&
                        g_depth_read_renderbuffer != 0 &&
                        g_depth_read_width == width &&
                        g_depth_read_height == height;
    if (reuse_source) {
        source_fbo = g_depth_read_fbo;
        image_rb = g_depth_read_renderbuffer;
        g_depth_read_reuse_calls++;
        if (g_depth_read_reuse_calls == 1 || g_depth_read_reuse_calls % 120 == 0) {
            LOGI("depth read resources reused: call=%llu fbo=%u rb=%u size=%dx%d image=%p",
                 (unsigned long long)g_depth_read_reuse_calls, source_fbo, image_rb,
                 width, height, (void *)image);
        }
    } else {
        if (g_depth_read_context == current_context) {
            if (g_depth_read_fbo != 0) {
                glDeleteFramebuffers(1, &g_depth_read_fbo);
            }
            if (g_depth_read_renderbuffer != 0) {
                glDeleteRenderbuffers(1, &g_depth_read_renderbuffer);
            }
        }
        g_depth_read_fbo = 0;
        g_depth_read_renderbuffer = 0;
        g_depth_read_image = EGL_NO_IMAGE_KHR;

        glGenFramebuffers(1, &source_fbo);
        glGenRenderbuffers(1, &image_rb);
        glBindRenderbuffer(GL_RENDERBUFFER, image_rb);
        image_target(GL_RENDERBUFFER, image);
        GLenum image_error = glGetError();
        if (image_error != GL_NO_ERROR) {
            LOGE("glEGLImageTargetRenderbufferStorageOES failed: 0x%x", image_error);
            glDeleteRenderbuffers(1, &image_rb);
            glDeleteFramebuffers(1, &source_fbo);
            goto cleanup;
        }
        g_depth_read_context = current_context;
        g_depth_read_image = image;
        g_depth_read_fbo = source_fbo;
        g_depth_read_renderbuffer = image_rb;
        g_depth_read_width = width;
        g_depth_read_height = height;
    }
    glBindFramebuffer(GL_FRAMEBUFFER, source_fbo);
    if (!reuse_source) {
        glFramebufferRenderbuffer(GL_FRAMEBUFFER, GL_DEPTH_ATTACHMENT,
                                  GL_RENDERBUFFER, image_rb);
    }
    {
        GLenum no_color = GL_NONE;
        glDrawBuffers(1, &no_color);
        glReadBuffer(GL_NONE);
    }
    if (glCheckFramebufferStatus(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE) {
        LOGE("depth EGLImage source FBO incomplete: 0x%x",
             glCheckFramebufferStatus(GL_FRAMEBUFFER));
        goto cleanup;
    }

    {
        if (g_depth_raw_capacity < pixel_count) {
            free(g_depth_raw_buffer);
            g_depth_raw_buffer = (uint16_t *)malloc(pixel_count * sizeof(uint16_t));
            g_depth_raw_capacity = g_depth_raw_buffer != NULL ? pixel_count : 0;
        }
        if (g_depth_raw_buffer != NULL) {
            glReadBuffer(GL_NONE);
            glReadPixels(0, 0, width, height, GL_DEPTH_COMPONENT,
                         GL_UNSIGNED_SHORT, g_depth_raw_buffer);
            read_error = glGetError();
            if (read_error == GL_NO_ERROR) {
                const float inv_depth_max = 1.0f / 65535.0f;
                for (size_t i = 0; i < pixel_count; ++i) {
                    depth[i] = (float)g_depth_raw_buffer[i] * inv_depth_max;
                }
                read_ok = true;
            }
        }
    }
    if (read_ok) goto cleanup;

    glGenTextures(1, &depth_texture);
    glBindTexture(GL_TEXTURE_2D, depth_texture);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
    glTexImage2D(GL_TEXTURE_2D, 0, GL_DEPTH_COMPONENT16, width, height, 0,
                 GL_DEPTH_COMPONENT, GL_UNSIGNED_SHORT, NULL);
    if (glGetError() != GL_NO_ERROR) {
        LOGE("depth texture allocation failed");
        goto cleanup;
    }
    glGenFramebuffers(1, &blit_fbo);
    glBindFramebuffer(GL_FRAMEBUFFER, blit_fbo);
    glFramebufferTexture2D(GL_FRAMEBUFFER, GL_DEPTH_ATTACHMENT,
                           GL_TEXTURE_2D, depth_texture, 0);
    {
        GLenum no_color = GL_NONE;
        glDrawBuffers(1, &no_color);
        glReadBuffer(GL_NONE);
    }
    if (glCheckFramebufferStatus(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE) {
        LOGE("depth texture FBO incomplete");
        goto cleanup;
    }

    glBindFramebuffer(GL_READ_FRAMEBUFFER, source_fbo);
    glReadBuffer(GL_NONE);
    glBindFramebuffer(GL_DRAW_FRAMEBUFFER, blit_fbo);
    {
        GLenum no_color = GL_NONE;
        glDrawBuffers(1, &no_color);
    }
    glBlitFramebuffer(0, 0, width, height, 0, 0, width, height,
                      GL_DEPTH_BUFFER_BIT, GL_NEAREST);
    GLenum blit_error = glGetError();
    if (blit_error != GL_NO_ERROR) {
        g_depth_blit_failures++;
        if (g_depth_blit_failures == 1 || g_depth_blit_failures % 120 == 0) {
            GLint read_buffer = -1;
            GLint draw_buffer = -1;
            GLint depth_bits = -1;
            GLint object_type = -1;
            GLint samples = -1;
            glBindFramebuffer(GL_READ_FRAMEBUFFER, source_fbo);
            glGetIntegerv(GL_READ_BUFFER, &read_buffer);
            glGetFramebufferAttachmentParameteriv(
                GL_READ_FRAMEBUFFER, GL_DEPTH_ATTACHMENT,
                GL_FRAMEBUFFER_ATTACHMENT_DEPTH_SIZE, &depth_bits);
            glGetFramebufferAttachmentParameteriv(
                GL_READ_FRAMEBUFFER, GL_DEPTH_ATTACHMENT,
                GL_FRAMEBUFFER_ATTACHMENT_OBJECT_TYPE, &object_type);
            glBindRenderbuffer(GL_RENDERBUFFER, image_rb);
            glGetRenderbufferParameteriv(GL_RENDERBUFFER, GL_RENDERBUFFER_SAMPLES,
                                         &samples);
            glBindFramebuffer(GL_DRAW_FRAMEBUFFER, blit_fbo);
            glGetIntegerv(GL_DRAW_BUFFER0, &draw_buffer);
            LOGE("depth EGLImage blit failed: 0x%x count=%llu "
                 "read_buffer=0x%x draw_buffer=0x%x depth_bits=%d "
                 "object_type=0x%x samples=%d",
                 blit_error, (unsigned long long)g_depth_blit_failures,
                 (unsigned)read_buffer, (unsigned)draw_buffer, depth_bits,
                 (unsigned)object_type, samples);
        }
        goto cleanup;
    }

    glGenTextures(1, &color_texture);
    glBindTexture(GL_TEXTURE_2D, color_texture);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
    glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA8, width, height, 0,
                 GL_RGBA, GL_UNSIGNED_BYTE, NULL);
    glGenFramebuffers(1, &color_fbo);
    glBindFramebuffer(GL_FRAMEBUFFER, color_fbo);
    glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0,
                           GL_TEXTURE_2D, color_texture, 0);
    {
        GLenum color_attachment = GL_COLOR_ATTACHMENT0;
        glDrawBuffers(1, &color_attachment);
        glReadBuffer(GL_COLOR_ATTACHMENT0);
    }
    if (glCheckFramebufferStatus(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE) {
        LOGE("depth pack FBO incomplete");
        goto cleanup;
    }

    vertex_shader = compile_depth_shader(GL_VERTEX_SHADER, vertex_source);
    fragment_shader = compile_depth_shader(GL_FRAGMENT_SHADER, fragment_source);
    if (vertex_shader == 0 || fragment_shader == 0) goto cleanup;
    program = glCreateProgram();
    glAttachShader(program, vertex_shader);
    glAttachShader(program, fragment_shader);
    glLinkProgram(program);
    GLint linked = 0;
    glGetProgramiv(program, GL_LINK_STATUS, &linked);
    if (!linked) {
        GLchar log[512] = {0};
        glGetProgramInfoLog(program, sizeof(log), NULL, log);
        LOGE("depth pack program link failed: %s", log);
        goto cleanup;
    }

    glGenVertexArrays(1, &vao);
    glBindVertexArray(vao);
    glUseProgram(program);
    glActiveTexture(GL_TEXTURE0);
    glBindTexture(GL_TEXTURE_2D, depth_texture);
    glUniform1i(glGetUniformLocation(program, "uDepth"), 0);
    glDisable(GL_DEPTH_TEST);
    glDisable(GL_STENCIL_TEST);
    glDisable(GL_BLEND);
    glDisable(GL_CULL_FACE);
    glDisable(GL_SCISSOR_TEST);
    glViewport(0, 0, width, height);
    glDrawArrays(GL_TRIANGLES, 0, 3);
    GLenum render_error = glGetError();
    if (render_error != GL_NO_ERROR) {
        LOGE("depth pack draw failed: 0x%x", render_error);
        goto cleanup;
    }

    packed = (uint8_t *)malloc(pixel_count * 4);
    if (packed == NULL) goto cleanup;
    glReadPixels(0, 0, width, height, GL_RGBA, GL_UNSIGNED_BYTE, packed);
    read_error = glGetError();
    if (read_error == GL_NO_ERROR) {
        for (size_t i = 0; i < pixel_count; ++i) {
            const uint8_t *p = packed + i * 4;
            uint32_t bits = ((uint32_t)p[0] << 24) | ((uint32_t)p[1] << 16) |
                            ((uint32_t)p[2] << 8) | (uint32_t)p[3];
            memcpy(&depth[i], &bits, sizeof(bits));
        }
        read_ok = true;
    }

cleanup:
    if (packed != NULL) free(packed);
    glBindFramebuffer(GL_FRAMEBUFFER, (GLuint)previous_fbo);
    glViewport(previous_viewport[0], previous_viewport[1],
               previous_viewport[2], previous_viewport[3]);
    glUseProgram(0);
    glBindVertexArray(0);
    if (program != 0) glDeleteProgram(program);
    if (vertex_shader != 0) glDeleteShader(vertex_shader);
    if (fragment_shader != 0) glDeleteShader(fragment_shader);
    if (vao != 0) glDeleteVertexArrays(1, &vao);
    if (color_texture != 0) glDeleteTextures(1, &color_texture);
    if (depth_texture != 0) glDeleteTextures(1, &depth_texture);
    if (color_fbo != 0) glDeleteFramebuffers(1, &color_fbo);
    if (blit_fbo != 0) glDeleteFramebuffers(1, &blit_fbo);
    if (!read_ok) {
        g_depth_read_failures++;
        if (g_depth_read_failures == 1 || g_depth_read_failures % 120 == 0) {
            LOGE("depth pack read failed: count=%llu error=0x%x",
                 (unsigned long long)g_depth_read_failures, read_error);
        }
    }
    return read_ok;
}

void ds_depth_image_cache_reset(void) {
    if (g_cached_depth_image == EGL_NO_IMAGE_KHR) return;
    PFN_ds_eglDestroyImageKHR destroy_image =
        (PFN_ds_eglDestroyImageKHR)eglGetProcAddress("eglDestroyImageKHR");
    if (g_cached_depth_display != EGL_NO_DISPLAY && destroy_image != NULL) {
        destroy_image(g_cached_depth_display, g_cached_depth_image);
    }
    g_cached_depth_image = EGL_NO_IMAGE_KHR;
    g_cached_depth_display = EGL_NO_DISPLAY;
    g_cached_depth_context = EGL_NO_CONTEXT;
    g_cached_depth_renderbuffer = 0;
    g_cached_depth_width = 0;
    g_cached_depth_height = 0;
}

void ds_depth_image_destroy(void *opaque_image) {
    if (opaque_image == NULL) return;
    EGLDisplay display = eglGetCurrentDisplay();
    PFN_ds_eglDestroyImageKHR destroy_image =
        (PFN_ds_eglDestroyImageKHR)eglGetProcAddress("eglDestroyImageKHR");
    if (display != EGL_NO_DISPLAY && destroy_image != NULL) {
        destroy_image(display, (EGLImageKHR)opaque_image);
    }
}

bool ds_ahb_bridge_fsr3_copy_fg(void) {
    if (!g_initialized || g_output_write_index < 0 ||
        g_output_write_index >= DS_OUTPUT_BUFFER_COUNT || !g_fsr3_copy_fg) {
        return false;
    }
    int fg_index = (g_output_write_index + 1) % DS_OUTPUT_BUFFER_COUNT;
    bool ok = g_fsr3_copy_fg(g_output_buffers[fg_index]);
    if (ok) g_output_write_index = fg_index;
    return ok;
}
bool ds_ahb_bridge_init(int width, int height, int output_width, int output_height) {
    if (g_initialized) {
        if (width == g_width && height == g_height &&
            output_width == g_output_width && output_height == g_output_height) {
            return true;
        }
        ds_ahb_bridge_destroy();
    }
    if (width <= 0 || height <= 0 || output_width <= 0 || output_height <= 0) {
        LOGE("invalid AHB bridge dimensions: %dx%d -> %dx%d", width, height,
             output_width, output_height);
        return false;
    }

    AHardwareBuffer_Desc desc = {0};
    desc.width = (uint32_t)width;
    desc.height = (uint32_t)height;
    desc.layers = 1;
    desc.format = AHARDWAREBUFFER_FORMAT_R8G8B8A8_UNORM;
    desc.usage = AHARDWAREBUFFER_USAGE_GPU_SAMPLED_IMAGE |
                 AHARDWAREBUFFER_USAGE_GPU_COLOR_OUTPUT |
                 AHARDWAREBUFFER_USAGE_CPU_READ_OFTEN;
    AhbApi *api = ahb_api();
    if (api == NULL) {
        LOGE("AHardwareBuffer API unavailable");
        return false;
    }
    int alloc_result = api->allocate(&desc, &g_buffer);
    if (alloc_result != 0 || g_buffer == NULL) {
        LOGE("AHardwareBuffer_allocate failed: %d", alloc_result);
        return false;
    }

    AHardwareBuffer_Desc output_desc = desc;
    output_desc.width = (uint32_t)output_width;
    output_desc.height = (uint32_t)output_height;
    output_desc.usage = AHARDWAREBUFFER_USAGE_GPU_SAMPLED_IMAGE |
                        AHARDWAREBUFFER_USAGE_GPU_COLOR_OUTPUT;
    for (int i = 0; i < DS_OUTPUT_BUFFER_COUNT; ++i) {
        int output_alloc_result = api->allocate(&output_desc, &g_output_buffers[i]);
        if (output_alloc_result != 0 || g_output_buffers[i] == NULL) {
            LOGE("output AHardwareBuffer_allocate failed index=%d result=%d",
                 i, output_alloc_result);
            ds_ahb_bridge_destroy();
            return false;
        }
    }

    EGLDisplay display = eglGetCurrentDisplay();
    if (display == EGL_NO_DISPLAY) {
        LOGE("eglGetCurrentDisplay failed: 0x%x", eglGetError());
        api->release(g_buffer);
        g_buffer = NULL;
        return false;
    }

    PFNEGLGETNATIVECLIENTBUFFERANDROIDPROC get_native =
        (PFNEGLGETNATIVECLIENTBUFFERANDROIDPROC)eglGetProcAddress(
            "eglGetNativeClientBufferANDROID");
    PFNEGLCREATEIMAGEKHRPROC create_image =
        (PFNEGLCREATEIMAGEKHRPROC)eglGetProcAddress("eglCreateImageKHR");
    PFN_glEGLImageTargetTexture2DOES image_target =
        (PFN_glEGLImageTargetTexture2DOES)eglGetProcAddress(
            "glEGLImageTargetTexture2DOES");
    if (!get_native || !create_image || !image_target) {
        LOGE("required EGLImage entry points missing");
        api->release(g_buffer);
        g_buffer = NULL;
        return false;
    }

    EGLClientBuffer client = get_native(g_buffer);
    if (!client) {
        LOGE("eglGetNativeClientBufferANDROID returned null: 0x%x", eglGetError());
        api->release(g_buffer);
        g_buffer = NULL;
        return false;
    }
    const EGLint image_attribs[] = {EGL_NONE};
    g_image = create_image(display, EGL_NO_CONTEXT, EGL_NATIVE_BUFFER_ANDROID,
                           client, image_attribs);
    if (g_image == EGL_NO_IMAGE_KHR) {
        LOGE("eglCreateImageKHR failed: 0x%x", eglGetError());
        api->release(g_buffer);
        g_buffer = NULL;
        return false;
    }

    glGenTextures(1, &g_texture);
    GLint previous_texture = 0;
    glGetIntegerv(GL_TEXTURE_BINDING_2D, &previous_texture);
    glBindTexture(GL_TEXTURE_2D, g_texture);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
    image_target(GL_TEXTURE_2D, g_image);
    GLenum gl_error = glGetError();
    glBindTexture(GL_TEXTURE_2D, (GLuint)previous_texture);
    if (gl_error != GL_NO_ERROR) {
        LOGE("glEGLImageTargetTexture2DOES failed: 0x%x", gl_error);
        glDeleteTextures(1, &g_texture);
        g_texture = 0;
        api->release(g_buffer);
        g_buffer = NULL;
        return false;
    }

    for (int i = 0; i < DS_OUTPUT_BUFFER_COUNT; ++i) {
        EGLClientBuffer output_client = get_native(g_output_buffers[i]);
        if (!output_client) {
            LOGE("output eglGetNativeClientBufferANDROID failed index=%d: 0x%x",
                 i, eglGetError());
            ds_ahb_bridge_destroy();
            return false;
        }
        g_output_images[i] = create_image(display, EGL_NO_CONTEXT,
            EGL_NATIVE_BUFFER_ANDROID, output_client, image_attribs);
        if (g_output_images[i] == EGL_NO_IMAGE_KHR) {
            LOGE("output eglCreateImageKHR failed index=%d: 0x%x", i, eglGetError());
            ds_ahb_bridge_destroy();
            return false;
        }
        glGenTextures(1, &g_output_textures[i]);
        glBindTexture(GL_TEXTURE_2D, g_output_textures[i]);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
        image_target(GL_TEXTURE_2D, g_output_images[i]);
        GLenum output_gl_error = glGetError();
        if (output_gl_error != GL_NO_ERROR) {
            LOGE("output glEGLImageTargetTexture2DOES failed index=%d: 0x%x",
                 i, output_gl_error);
            glBindTexture(GL_TEXTURE_2D, (GLuint)previous_texture);
            ds_ahb_bridge_destroy();
            return false;
        }
    }
    glBindTexture(GL_TEXTURE_2D, (GLuint)previous_texture);

    g_width = width;
    g_height = height;
    g_output_width = output_width;
    g_output_height = output_height;
    g_initialized = true;
    LOGI("AHB bridge initialized: %dx%d -> %dx%d texture=%u inputBuffer=%p outputs=%p,%p textures=%u,%u",
         width, height, output_width, output_height, g_texture, (void *)g_buffer,
         (void *)g_output_buffers[0], (void *)g_output_buffers[1],
         g_output_textures[0], g_output_textures[1]);
    return true;
}

bool ds_ahb_bridge_copy_from_texture(GLuint source_texture, int width, int height) {
    if (!g_initialized || width != g_width || height != g_height) {
        return false;
    }

    if (g_source_fbo == 0) {
        glGenFramebuffers(1, &g_source_fbo);
    }
    GLint previous_fbo = 0;
    glGetIntegerv(GL_FRAMEBUFFER_BINDING, &previous_fbo);
    GLint previous_texture = 0;
    glGetIntegerv(GL_TEXTURE_BINDING_2D, &previous_texture);
    glBindFramebuffer(GL_FRAMEBUFFER, g_source_fbo);
    glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D,
                           source_texture, 0);
    GLenum fbo_status = glCheckFramebufferStatus(GL_FRAMEBUFFER);
    GLenum attach_error = glGetError();
    if (fbo_status != GL_FRAMEBUFFER_COMPLETE || attach_error != GL_NO_ERROR) {
        LOGE("source FBO incomplete: status=0x%x error=0x%x", fbo_status, attach_error);
        glBindFramebuffer(GL_FRAMEBUFFER, (GLuint)previous_fbo);
        glBindTexture(GL_TEXTURE_2D, (GLuint)previous_texture);
        return false;
    }
    struct timespec copy_start;
    bool copy_timing_valid = clock_gettime(CLOCK_MONOTONIC, &copy_start) == 0;
    glBindTexture(GL_TEXTURE_2D, g_texture);
    glCopyTexSubImage2D(GL_TEXTURE_2D, 0, 0, 0, 0, 0, width, height);
    // The Vulkan consumer is a separate API on the same thread. Finish the
    // GLES write before importing the AHB for the next FSR3 dispatch.
    glFinish();
    if (copy_timing_valid) {
        struct timespec copy_end;
        if (clock_gettime(CLOCK_MONOTONIC, &copy_end) == 0) {
            const double elapsed_ms =
                (copy_end.tv_sec - copy_start.tv_sec) * 1000.0 +
                (copy_end.tv_nsec - copy_start.tv_nsec) / 1000000.0;
            g_copy_timing_total_ms += elapsed_ms;
            g_copy_timing_calls++;
            if (g_copy_timing_calls == 1 || g_copy_timing_calls % 120 == 0) {
                LOGI("GLES color copy+finish timing: avg %.2fms over %llu frames",
                     g_copy_timing_total_ms / g_copy_timing_calls,
                     (unsigned long long)g_copy_timing_calls);
            }
        }
    }
    GLenum gl_error = glGetError();
    glBindTexture(GL_TEXTURE_2D, (GLuint)previous_texture);
    glBindFramebuffer(GL_FRAMEBUFFER, (GLuint)previous_fbo);
    if (gl_error != GL_NO_ERROR) {
        LOGE("glCopyTexSubImage2D to AHB failed: 0x%x", gl_error);
        return false;
    }
    return true;
}

bool ds_ahb_bridge_verify_pixel(uint8_t out_rgba[4]) {
    if (!g_initialized || out_rgba == NULL) return false;
    void *data = NULL;
    AhbApi *api = ahb_api();
    if (api == NULL) return false;
    int result = api->lock(g_buffer, AHARDWAREBUFFER_USAGE_CPU_READ_OFTEN,
                           -1, NULL, &data);
    if (result != 0 || data == NULL) {
        LOGE("AHardwareBuffer_lock failed: %d", result);
        return false;
    }
    AHardwareBuffer_Desc desc;
    api->describe(g_buffer, &desc);
    const uint8_t *pixels = (const uint8_t *)data;
    const size_t stride = desc.stride > 0 ? desc.stride : desc.width;
    const size_t offset = ((size_t)(desc.height / 2) * stride + desc.width / 2) * 4;
    memcpy(out_rgba, pixels + offset, 4);
    api->unlock(g_buffer, NULL);
    return true;
}

void ds_ahb_bridge_destroy(void) {
    if (g_fsr3_library != NULL) {
        PFN_ds_fsr3_shutdown_ahb shutdown =
            (PFN_ds_fsr3_shutdown_ahb)dlsym(g_fsr3_library, "ds_fsr3_shutdown_ahb");
        if (shutdown != NULL) shutdown();
    }
    if (g_source_fbo != 0) {
        glDeleteFramebuffers(1, &g_source_fbo);
        g_source_fbo = 0;
    }
    if (g_texture != 0) {
        glDeleteTextures(1, &g_texture);
        g_texture = 0;
    }
    for (int i = 0; i < DS_OUTPUT_BUFFER_COUNT; ++i) {
        if (g_output_textures[i] != 0) {
            glDeleteTextures(1, &g_output_textures[i]);
            g_output_textures[i] = 0;
        }
    }
    EGLDisplay display = eglGetCurrentDisplay();
    PFNEGLDESTROYIMAGEKHRPROC destroy_image =
        (PFNEGLDESTROYIMAGEKHRPROC)eglGetProcAddress("eglDestroyImageKHR");
    if (g_image != EGL_NO_IMAGE_KHR) {
        if (display != EGL_NO_DISPLAY && destroy_image) {
            destroy_image(display, g_image);
        }
        g_image = EGL_NO_IMAGE_KHR;
    }
    for (int i = 0; i < DS_OUTPUT_BUFFER_COUNT; ++i) {
        if (g_output_images[i] != EGL_NO_IMAGE_KHR) {
            if (display != EGL_NO_DISPLAY && destroy_image) {
                destroy_image(display, g_output_images[i]);
            }
            g_output_images[i] = EGL_NO_IMAGE_KHR;
        }
    }
    AhbApi *api = ahb_api();
    if (g_buffer != NULL) {
        if (api != NULL) api->release(g_buffer);
        g_buffer = NULL;
    }
    for (int i = 0; i < DS_OUTPUT_BUFFER_COUNT; ++i) {
        if (g_output_buffers[i] != NULL) {
            if (api != NULL) api->release(g_output_buffers[i]);
            g_output_buffers[i] = NULL;
        }
    }
    g_output_write_index = -1;
    g_width = 0;
    g_height = 0;
    g_output_width = 0;
    g_output_height = 0;
    g_initialized = false;
}
