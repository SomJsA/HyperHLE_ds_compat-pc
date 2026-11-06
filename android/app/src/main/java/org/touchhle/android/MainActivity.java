/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 *
 * Parts of this file are derived from SDL 2's Android project template, which
 * has a different license. Please see vendor/SDL/LICENSE.txt for details.
 */
package org.touchhle.android;

import android.app.AlertDialog;
import android.content.Intent;
import android.content.res.AssetManager;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.system.Os;
import android.os.Bundle;
import android.util.Log;
import android.view.Gravity;
import android.view.KeyEvent;
import android.view.View;
import android.view.Window;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.view.WindowManager;
import android.widget.RelativeLayout;
import android.widget.TextView;
import android.widget.Toast;

import org.libsdl.app.SDLActivity;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;

/**
 * A wrapper class over SDLActivity
 */

public class MainActivity extends SDLActivity {
    private static final String TAG = "touchHLE";

    private final Runnable immersiveModeRunnable = this::applyImmersiveMode;
    private boolean showFps;
    private boolean logStats;
    private int statsLogTick;
    private Handler fpsHandler;
    private TextView fpsOverlay;
    private AlertDialog pauseDialog;
    private boolean manualPause;
    private final Runnable fpsUpdateRunnable = this::updateFpsOverlay;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        showFps = getIntent().getBooleanExtra("show_fps", true);
        logStats = getIntent().getBooleanExtra("stats_log", false);
        applyImmersiveMode();
        scheduleImmersiveMode();
        if (showFps) {
            installFpsOverlay();
        }
    }

    @Override
    protected void onResume() {
        super.onResume();
        if (manualPause) {
            pauseNativeThread();
        }
        scheduleImmersiveMode();
        if (showFps || logStats) {
            startFpsUpdates();
        }
    }

    @Override
    protected void onPause() {
        stopFpsUpdates();
        super.onPause();
    }

    @Override
    protected void onDestroy() {
        stopFpsUpdates();
        if (pauseDialog != null) {
            pauseDialog.dismiss();
            pauseDialog = null;
        }
        scheduleGameProcessExit();
        // SDL onDestroy() sends SDL_QUIT and then waits for the native thread.
        // If our in-game pause menu still owns the native pause semaphore, that
        // thread cannot consume SDL_QUIT and the UI thread blocks in join().
        // Release the manual pause before delegating teardown to SDL.
        resumeFromMenu();
        super.onDestroy();
    }

    private void scheduleGameProcessExit() {
        // Death Space's termination delegate can re-enter the guest scheduler
        // and never return to std::process::exit(). SDL then waits forever in
        // onDestroy(). MainActivity runs in the dedicated :game process, so
        // give the guest a short chance to flush state, then reap only that
        // process. LauncherActivity remains responsive in the default process.
        Thread exitThread = new Thread(() -> {
            try {
                Thread.sleep(2000);
            } catch (InterruptedException ignored) {
                Thread.currentThread().interrupt();
            }
            android.os.Process.killProcess(android.os.Process.myPid());
        }, "touchHLE-game-exit");
        exitThread.setDaemon(true);
        exitThread.start();
    }

    @Override
    public void onBackPressed() {
        if (pauseDialog != null && pauseDialog.isShowing()) {
            return;
        }
        pauseForMenu();
        showPauseMenu();
    }

    @Override
    public boolean dispatchKeyEvent(KeyEvent event) {
        if (event.getKeyCode() == KeyEvent.KEYCODE_BACK) {
            if (event.getAction() == KeyEvent.ACTION_UP
                    && event.getRepeatCount() == 0) {
                onBackPressed();
            }
            return true;
        }
        return super.dispatchKeyEvent(event);
    }

    private void pauseForMenu() {
        if (!manualPause) {
            manualPause = true;
            pauseNativeThread();
        }
    }

    private void resumeFromMenu() {
        if (manualPause) {
            manualPause = false;
            resumeNativeThread();
        }
    }

    private void showPauseMenu() {
        AlertDialog dialog = new AlertDialog.Builder(this)
                .setTitle("游戏已暂停")
                .setMessage("返回键不会直接退出游戏。"
                        + "返回启动器或退出游戏前都会再次确认。")
                .setPositiveButton("继续游戏", (d, which) -> resumeFromMenu())
                .setNeutralButton("返回启动器", (d, which) -> returnToLauncher())
                .setNegativeButton("退出游戏", (d, which) -> confirmExitGame())
                .setOnCancelListener(d -> resumeFromMenu())
                .create();
        pauseDialog = dialog;
        dialog.show();
    }

    private void confirmExitGame() {
        new AlertDialog.Builder(this)
                .setTitle("退出游戏？")
                .setMessage("未写入存档的进度可能丢失。确认退出到桌面？")
                .setPositiveButton("退出", (d, which) -> {
                    resumeFromMenu();
                    nativeSendQuit();
                    finish();
                })
                .setNegativeButton("取消", (d, which) -> showPauseMenu())
                .setOnCancelListener(d -> showPauseMenu())
                .show();
    }

    private void returnToLauncher() {
        resumeFromMenu();
        startActivity(new Intent(this, LauncherActivity.class));
        finish();
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus) {
            scheduleImmersiveMode();
        }
    }

    @Override
    public void onSystemUiVisibilityChange(int visibility) {
        super.onSystemUiVisibilityChange(visibility);
        if ((visibility & View.SYSTEM_UI_FLAG_FULLSCREEN) == 0) {
            scheduleImmersiveMode();
        }
    }

    /**
     * SDL asks Android for a non-fullscreen window after the activity's first
     * immersive-mode pass. Re-assert after the decor view is attached and once
     * more after the native window settles, otherwise Android 15 leaves the
     * status bar visible on top of the game.
     */
    private void scheduleImmersiveMode() {
        View decor = getWindow().getDecorView();
        decor.removeCallbacks(immersiveModeRunnable);
        decor.post(immersiveModeRunnable);
        decor.postDelayed(immersiveModeRunnable, 250);
        decor.postDelayed(immersiveModeRunnable, 1000);
        decor.postDelayed(immersiveModeRunnable, 2000);
    }

    @SuppressWarnings("deprecation")
    private void applyImmersiveMode() {
        Window window = getWindow();
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON
                | WindowManager.LayoutParams.FLAG_FULLSCREEN);
        window.clearFlags(WindowManager.LayoutParams.FLAG_FORCE_NOT_FULLSCREEN);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            WindowManager.LayoutParams attributes = window.getAttributes();
            attributes.layoutInDisplayCutoutMode =
                    WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;
            window.setAttributes(attributes);
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            window.setDecorFitsSystemWindows(false);
            WindowInsetsController controller = window.getInsetsController();
            if (controller != null) {
                controller.hide(WindowInsets.Type.systemBars());
                controller.setSystemBarsBehavior(
                        WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
            }
        }
        View decor = window.getDecorView();
        decor.setSystemUiVisibility(
                View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                        | View.SYSTEM_UI_FLAG_FULLSCREEN
                        | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                        | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                        | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                        | View.SYSTEM_UI_FLAG_LAYOUT_STABLE);
        decor.requestApplyInsets();
    }
    private static final String BUNDLED_IPA_ASSET = "touchHLE_bundled/DeadSpace.ipa";
    private static final String BUNDLED_IPA_ID_ASSET = "touchHLE_bundled/bundle.id";
    private static final String BUNDLED_IPA_FILENAME = "DeadSpace.ipa";

    @Override
    protected String[] getLibraries() {
        return new String[]{
            "SDL2",
            "touchHLE"
        };
    }

    /**
     * The Android release carries its game payload inside the APK. Copy it
     * once to the app's external files directory, then hand the absolute path
     * to touchHLE so the normal iOS bundle/IPA loader opens it directly.
     */
    @Override
    protected String[] getArguments() {
        try {
            File ipa = null;
            String requestedIpaPath = getIntent().getStringExtra("ipa_path");
            if (requestedIpaPath != null && !requestedIpaPath.trim().isEmpty()) {
                File requestedIpa = new File(requestedIpaPath);
                if (requestedIpa.isFile() && requestedIpa.canRead()) {
                    ipa = requestedIpa;
                    Log.i(TAG, "Launching IPA from explicit path: " + ipa.getAbsolutePath());
                } else {
                    Log.w(TAG, "Ignoring unreadable IPA path: " + requestedIpaPath);
                }
            }
            if (ipa == null) {
                ipa = ensureBundledIpaCopied();
            }
            if (ipa != null) {
                Log.i(TAG, "Launching IPA: " + ipa.getAbsolutePath());
                // Dead Space ships Simplified Chinese assets as CHS_CN. Android reports
                // the region language as bare "zh"; provide explicit BCP-47 tags so
                // the game selects the Chinese string table on a fresh profile.
                int scale = getIntent().getIntExtra("scale_hack", BuildConfig.SCALE_HACK);
                scale = Math.max(1, Math.min(3, scale));
                int fpsCap = getIntent().getIntExtra("fps_cap", 0);
                boolean debugLogs = getIntent().getBooleanExtra("debug_logs", false);
                boolean dumpFrames = getIntent().getBooleanExtra("dump_frames", false);
                boolean pcCompat = getIntent().getBooleanExtra("pc_compat", true);
                boolean fastPresent = getIntent().getBooleanExtra(
                        "fast_present", BuildConfig.FAST_PRESENT);
                boolean skipReadback = getIntent().getBooleanExtra("skip_readback", false);
                boolean gpuPresentCopy = getIntent().getBooleanExtra("gpu_present_copy", false);
                int fsrMode = getIntent().getIntExtra("present_fsr", 0);
                boolean aaEnabled = getIntent().getBooleanExtra("aa", false);
                boolean stretchViewport = getIntent().getBooleanExtra("viewport_stretch", false);
                int sharpenTenths = getIntent().getIntExtra("present_sharpen", 0);
                boolean fsr3AhbProbe = getIntent().getBooleanExtra("fsr3_ahb_probe", false);
                boolean fsr3Force = getIntent().getBooleanExtra("fsr3_force", false);
                boolean fsr3FrameGeneration = getIntent().getBooleanExtra("fsr3_fg", false);
                boolean fsr3RealDepth = getIntent().getBooleanExtra("fsr3_real_depth", false);
                boolean fsr3GpuDepth = getIntent().getBooleanExtra("fsr3_gpu_depth", false);
                boolean fsr3Async = getIntent().getBooleanExtra("fsr3_async", false);
                boolean fsr3DirectOutput = getIntent().getBooleanExtra(
                        "fsr3_direct_output", false);
                boolean fsr3DiagReadback = getIntent().getBooleanExtra(
                        "fsr3_diag_readback", false);
                boolean fsr3Trace = getIntent().getBooleanExtra("fsr3_trace", false);
                boolean fsr3FgSynthetic = getIntent().getBooleanExtra(
                        "fsr3_fg_synthetic", false);
                boolean fsr3FgProbe = getIntent().getBooleanExtra("fsr3_fg_probe", false);
                configureRuntimeOptions(
                        scale, fpsCap, debugLogs, dumpFrames, pcCompat,
                        skipReadback, fsrMode, aaEnabled, sharpenTenths, stretchViewport,
                        gpuPresentCopy, fsr3AhbProbe, fsr3FrameGeneration, fsr3RealDepth,
                        fsr3GpuDepth, fsr3Async, fsr3DirectOutput, fsr3DiagReadback,
                        fsr3Trace, fsr3Force,
                        fsr3FgSynthetic, fsr3FgProbe);

                List<String> args = new ArrayList<>();
                args.add("--preferred-languages=zh-Hans,zh");
                args.add("--scale-hack=" + scale);
                args.add(fpsCap == 0 ? "--fps-limit=off" : "--fps-limit=" + fpsCap);
                if (debugLogs) {
                    args.add("--print-fps");
                }
                if (fastPresent) {
                    args.add("--fast-present");
                }
                args.add(ipa.getAbsolutePath());
                return args.toArray(new String[0]);
            }
        } catch (IOException e) {
            Log.e(TAG, "Could not prepare bundled IPA", e);
            runOnUiThread(() -> {
                Toast.makeText(
                        this,
                        "Could not prepare bundled game data. Check free storage and logcat.",
                        Toast.LENGTH_LONG
                ).show();
                finish();
            });
        }

        // Never expose touchHLE's desktop-style app picker in the Android
        // release. Surface the packaging error and return to the launcher.
        return new String[0];
    }

    private void configureRuntimeOptions(
            int scale, int fpsCap, boolean debugLogs, boolean dumpFrames,
            boolean pcCompat, boolean skipReadback, int fsrMode,
            boolean aaEnabled, int sharpenTenths, boolean stretchViewport,
            boolean gpuPresentCopy, boolean fsr3AhbProbe,
            boolean fsr3FrameGeneration, boolean fsr3RealDepth,
            boolean fsr3GpuDepth, boolean fsr3Async, boolean fsr3DirectOutput,
            boolean fsr3DiagReadback, boolean fsr3Trace, boolean fsr3Force,
            boolean fsr3FgSynthetic, boolean fsr3FgProbe) {
        File base = getExternalFilesDir(null);
        if (base == null) {
            base = getFilesDir();
        }
        File diagnostics = new File(base, "diagnostics");
        diagnostics.mkdirs();

        // PC mainline is authoritative for content rendering. These texture,
        // fog and depth repairs remain enabled in every Android quality
        // preset; compatibility mode only disables the opening projection fix
        // and lowers the guest screen scale.
        setEnv("TOUCHHLE_DS_DEPTH_SIZE", "24");
        setEnv("TOUCHHLE_DS_MSAA", "4");
        setEnv("TOUCHHLE_DS_MSAA_CLEAR_DEPTH", "1");
        unsetEnv("TOUCHHLE_DS_MSAA_PROBE");
        unsetEnv("TOUCHHLE_DS_MSAA_FBO_DEPTH");
        unsetEnv("TOUCHHLE_DS_MSAA_DRAWABLE_ONLY");
        setEnv("TOUCHHLE_DS_NO_FOG", "0");
        setEnv("TOUCHHLE_DS_NO_FOG_ORTHO", "1");
        setEnv("TOUCHHLE_DS_NO_FOG_SCREEN", "0");
        setEnv("TOUCHHLE_DS_NO_FOG_HASH", "7027023c,56c62e8a");
        unsetEnv("TOUCHHLE_DS_NO_FOG_HASH_COUNT");
        unsetEnv("TOUCHHLE_DS_NO_FOG_BLEND");

        // The asteroid d1feb687:600 must retain GL_LESS and depth writes.
        // Only full-screen background atlases use the scoped coplanar repair.
        unsetEnv("TOUCHHLE_DS_DEPTH_LEQUAL");
        setEnv("TOUCHHLE_DS_DEPTH_LEQUAL_HASH",
                "25a08a82,cee43546,36497762,1d5cccb8,8c12a704,87127d0f,"
                        + "e70efc97,2131afd5,18c949fb,f4d178fa");
        setEnv("TOUCHHLE_DS_DEPTH_LEQUAL_NOWRITE", "1");
        unsetEnv("TOUCHHLE_DS_DEPTH_ALWAYS_HASH_COUNT");
        unsetEnv("TOUCHHLE_DS_CLEAR_DEPTH_HASH_COUNT");
        unsetEnv("TOUCHHLE_DS_CLEAR_OPENING_DEPTH");
        unsetEnv("TOUCHHLE_DS_DST_FILL_HASH_COUNT");
        unsetEnv("TOUCHHLE_DS_NODEPTH_ALL");

        setEnv("TOUCHHLE_DS_NO_CULL_HASH_COUNT", "02d3486c:405");
        unsetEnv("TOUCHHLE_DS_FORCE_NO_CULL");
        unsetEnv("TOUCHHLE_DS_FLIP_WINDING_HASH_COUNT");
        unsetEnv("TOUCHHLE_DS_TEXNODEPTH");
        unsetEnv("TOUCHHLE_DS_TEXNODEPTH_HASH");
        setEnv("TOUCHHLE_DS_TEXNODEPTH_HASH_COUNT",
                "56c62e8a:6,56c62e8a:36,7027023c:6,7027023c:36");

        unsetEnv("TOUCHHLE_DS_TEXFILTER");
        unsetEnv("TOUCHHLE_DS_TEXFILTER_HASH");
        setEnv("TOUCHHLE_DS_TEXFILTER_LINEAR_HASH", "25a08a82");
        setEnv("TOUCHHLE_DS_TEXFILTER_LINEAR_MAX_COUNT", "6");
        setEnv("TOUCHHLE_DS_OPENING_FOG_FIX", "1");
        setEnv("TOUCHHLE_DS_BLOOD_DEPTH_FIX", "1");
        setEnv("TOUCHHLE_DS_BLOOD_SIZE", "0.5");
        unsetEnv("TOUCHHLE_DS_TEXWRAP_HASH");
        unsetEnv("TOUCHHLE_DS_TEXMAT_IDENTITY_HASH_COUNT");
        unsetEnv("TOUCHHLE_DS_PRETRANSFORM_TEX_HASH");
        setEnv("TOUCHHLE_DS_AUTO_OPENING_MIPS", "1");
        setEnv("TOUCHHLE_DS_AUTO_MIPS_ALL", "0");
        setEnv("TOUCHHLE_DS_TEX_UPLOAD_FINISH", "1");

        setEnv("TOUCHHLE_DS_CG_GLYPH_SCALE", "0.68");
        setEnv("TOUCHHLE_DS_CG_GLYPH_DY", "0");
        setEnv("TOUCHHLE_DS_ALPHA_AS_WHITE", "0");
        if (pcCompat) {
            unsetEnv("TOUCHHLE_DS_PROJ_Z_SCALE");
            unsetEnv("TOUCHHLE_DS_NO_PROJ_Z_FIX");
            setEnv("TOUCHHLE_DS_SCREEN_SCALE", "2");
        } else {
            unsetEnv("TOUCHHLE_DS_PROJ_Z_SCALE");
            setEnv("TOUCHHLE_DS_NO_PROJ_Z_FIX", "1");
            setEnv("TOUCHHLE_DS_SCREEN_SCALE", "1");
        }

        if (skipReadback) {
            setEnv("TOUCHHLE_DS_SKIP_READBACK_FINISH", "1");
        } else {
            unsetEnv("TOUCHHLE_DS_SKIP_READBACK_FINISH");
        }

        if (fsrMode > 0) {
            setEnv("TOUCHHLE_DS_PRESENT_FSR", Integer.toString(Math.min(2, fsrMode)));
        } else {
            unsetEnv("TOUCHHLE_DS_PRESENT_FSR");
        }
        if (aaEnabled) {
            setEnv("TOUCHHLE_DS_AA", "1");
        } else {
            unsetEnv("TOUCHHLE_DS_AA");
        }
        if (sharpenTenths > 0) {
            setEnv("TOUCHHLE_DS_PRESENT_SHARPEN",
                    String.format(Locale.US, "%.1f", sharpenTenths / 10.0));
        } else {
            unsetEnv("TOUCHHLE_DS_PRESENT_SHARPEN");
        }

        if (gpuPresentCopy) {
            setEnv("TOUCHHLE_DS_GPU_PRESENT_COPY", "1");
        } else {
            unsetEnv("TOUCHHLE_DS_GPU_PRESENT_COPY");
        }
        // The community FSR3 Vulkan port uses Vulkan 1.2 core entry points and
        // is only supported from Android 12 / API 31. AHardwareBuffer itself
        // exists earlier, but exposing the option on API 26-30 would only crash
        // or silently fall back after doing bridge work.
        boolean fsr3Capable = Fsr3Capabilities.supportsBasicVulkanPath(this);
        if (fsr3AhbProbe && (fsr3Capable || fsr3Force)) {
            setEnv("TOUCHHLE_DS_FSR3_AHB", "1");
            if (fsr3Force && !fsr3Capable) {
                setEnv("TOUCHHLE_DS_FSR3_FORCE_UNSUPPORTED", "1");
                Log.w(TAG, "FSR3 force-enable requested despite failed capability probe");
            } else {
                unsetEnv("TOUCHHLE_DS_FSR3_FORCE_UNSUPPORTED");
            }
            if (fsr3FrameGeneration) {
                setEnv("TOUCHHLE_DS_FSR3_FG", "1");
            } else {
                unsetEnv("TOUCHHLE_DS_FSR3_FG");
            }
            if (fsr3RealDepth) {
                setEnv("TOUCHHLE_DS_FSR3_REAL_DEPTH", "1");
            } else {
                unsetEnv("TOUCHHLE_DS_FSR3_REAL_DEPTH");
            }
            if (fsr3GpuDepth) {
                setEnv("TOUCHHLE_DS_FSR3_GPU_DEPTH", "1");
            } else {
                unsetEnv("TOUCHHLE_DS_FSR3_GPU_DEPTH");
            }
            if (fsr3Async) {
                setEnv("TOUCHHLE_DS_FSR3_ASYNC", "1");
            } else {
                unsetEnv("TOUCHHLE_DS_FSR3_ASYNC");
            }
            if (fsr3DirectOutput) {
                setEnv("TOUCHHLE_DS_FSR3_DIRECT_OUTPUT", "1");
            } else {
                unsetEnv("TOUCHHLE_DS_FSR3_DIRECT_OUTPUT");
            }
            if (fsr3DiagReadback) {
                setEnv("TOUCHHLE_DS_FSR3_DIAG_READBACK", "1");
            } else {
                unsetEnv("TOUCHHLE_DS_FSR3_DIAG_READBACK");
            }
            if (fsr3Trace) {
                setEnv("TOUCHHLE_DS_FSR3_TRACE", "1");
            } else {
                unsetEnv("TOUCHHLE_DS_FSR3_TRACE");
            }
            if (fsr3FrameGeneration && fsr3FgSynthetic) {
                setEnv("TOUCHHLE_DS_FSR3_FG_SYNTHETIC", "1");
            } else {
                unsetEnv("TOUCHHLE_DS_FSR3_FG_SYNTHETIC");
            }
            if (fsr3FrameGeneration && fsr3FgProbe) {
                setEnv("TOUCHHLE_DS_FSR3_FG_PROBE", "1");
            } else {
                unsetEnv("TOUCHHLE_DS_FSR3_FG_PROBE");
            }
        } else {
            unsetEnv("TOUCHHLE_DS_FSR3_AHB");
            unsetEnv("TOUCHHLE_DS_FSR3_FORCE_UNSUPPORTED");
            unsetEnv("TOUCHHLE_DS_FSR3_FG");
            unsetEnv("TOUCHHLE_DS_FSR3_REAL_DEPTH");
            unsetEnv("TOUCHHLE_DS_FSR3_GPU_DEPTH");
            unsetEnv("TOUCHHLE_DS_FSR3_ASYNC");
            unsetEnv("TOUCHHLE_DS_FSR3_DIRECT_OUTPUT");
            unsetEnv("TOUCHHLE_DS_FSR3_DIAG_READBACK");
            unsetEnv("TOUCHHLE_DS_FSR3_TRACE");
            unsetEnv("TOUCHHLE_DS_FSR3_FG_SYNTHETIC");
            unsetEnv("TOUCHHLE_DS_FSR3_FG_PROBE");
        }
        if (stretchViewport) {
            setEnv("TOUCHHLE_DS_VIEWPORT_STRETCH", "1");
        } else {
            unsetEnv("TOUCHHLE_DS_VIEWPORT_STRETCH");
        }

        if (debugLogs) {
            setEnv("TOUCHHLE_DS_LOG_FOG", "1");
            setEnv("TOUCHHLE_DS_PROJ_TRACE", "1");
            setEnv("TOUCHHLE_DS_DRAW_LOG", "120");
            setEnv("TOUCHHLE_DS_LOG_GLYPHS", "1");
            setEnv("TOUCHHLE_DS_LOG_SKIN", "1");
            setEnv("TOUCHHLE_DS_DEPTH_TRACE", "1");
            // Per-draw texture-coordinate tracing writes tens of lines per
            // frame and has caused launcher input ANRs plus multi-hundred-MB
            // logs. It remains available as an explicit environment override
            // for targeted diagnostics, never as part of general debug logs.
            unsetEnv("TOUCHHLE_DS_TEXTURE_TRACE");
            unsetEnv("TOUCHHLE_DS_PIXEL_TRACE");
            unsetEnv("TOUCHHLE_DS_TEXFLAT_HASH");
            unsetEnv("TOUCHHLE_DS_TEXNODEPTH_HASH");
        } else {
            unsetEnv("TOUCHHLE_DS_LOG_FOG");
            unsetEnv("TOUCHHLE_DS_PROJ_TRACE");
            unsetEnv("TOUCHHLE_DS_TEXTURE_TRACE");
            unsetEnv("TOUCHHLE_DS_DRAW_LOG");
            unsetEnv("TOUCHHLE_DS_LOG_GLYPHS");
            unsetEnv("TOUCHHLE_DS_LOG_SKIN");
            unsetEnv("TOUCHHLE_DS_DEPTH_TRACE");
            unsetEnv("TOUCHHLE_DS_PIXEL_TRACE");
            unsetEnv("TOUCHHLE_DS_TEXFLAT_HASH");
        }
        applyDiagnosticEnvOverrides(getIntent().getStringExtra("diagnostic_env"));

        if (dumpFrames) {
            File frames = new File(diagnostics, "frames");
            frames.mkdirs();
            setEnv("TOUCHHLE_DS_DUMP_FRAMES", frames.getAbsolutePath());
            setEnv("TOUCHHLE_DS_DUMP_FRAME_EVERY", "600");
        } else {
            unsetEnv("TOUCHHLE_DS_DUMP_FRAMES");
            unsetEnv("TOUCHHLE_DS_DUMP_FRAME_EVERY");
        }

        try (FileOutputStream output = new FileOutputStream(
                new File(diagnostics, "session.log"), true)) {
            String line = new java.util.Date()
                    + " scale=" + scale
                    + " cap=" + (fpsCap == 0 ? "unlimited" : fpsCap)
                    + " debug=" + debugLogs
                    + " dumps=" + dumpFrames
                    + " pcCompat=" + pcCompat
                    + " fsr=" + fsrMode
                    + " aa=" + aaEnabled
                    + " sharpen=" + sharpenTenths
                    + " stretch=" + stretchViewport
                    + " gpuCopy=" + gpuPresentCopy
                    + " fsr3Ahb=" + fsr3AhbProbe
                    + " fsr3Force=" + fsr3Force
                    + " fsr3Fg=" + fsr3FrameGeneration
                    + " fsr3Depth=" + fsr3RealDepth
                    + " fsr3Async=" + fsr3Async
                    + " fsr3Direct=" + fsr3DirectOutput
                    + " fsr3Diag=" + fsr3DiagReadback
                    + " fsr3Trace=" + fsr3Trace
                    + " fsr3FgSynthetic=" + fsr3FgSynthetic
                    + " fsr3FgProbe=" + fsr3FgProbe + "\n";
            output.write(line.getBytes(StandardCharsets.UTF_8));
            output.getFD().sync();
        } catch (IOException e) {
            Log.w(TAG, "Could not write diagnostics session log", e);
        }
    }

    private void applyDiagnosticEnvOverrides(String spec) {
        if (spec == null || spec.isEmpty() || spec.length() > 4096) {
            return;
        }
        for (String item : spec.split("@@")) {
            int equals = item.indexOf('=');
            if (equals <= 0) {
                continue;
            }
            String key = item.substring(0, equals).trim();
            String value = item.substring(equals + 1).trim();
            if (!key.startsWith("TOUCHHLE_DS_")) {
                Log.w(TAG, "Ignoring non-touchHLE diagnostic environment key: " + key);
                continue;
            }
            if ("__UNSET__".equals(value)) {
                unsetEnv(key);
            } else {
                setEnv(key, value);
            }
        }
    }

    private static void setEnv(String name, String value) {
        try {
            Os.setenv(name, value, true);
        } catch (Throwable t) {
            Log.w(TAG, "Could not set environment variable " + name, t);
        }
    }

    private static void unsetEnv(String name) {
        try {
            Os.unsetenv(name);
        } catch (Throwable t) {
            Log.w(TAG, "Could not unset environment variable " + name, t);
        }
    }

    private void installFpsOverlay() {
        if (fpsOverlay != null || mLayout == null) {
            return;
        }
        TextView view = new TextView(this);
        view.setText("FPS --\nframe --");
        view.setTextSize(14);
        view.setTextColor(Color.rgb(92, 232, 255));
        view.setTypeface(Typeface.MONOSPACE, Typeface.BOLD);
        view.setGravity(Gravity.START);
        view.setPadding(dp(12), dp(7), dp(12), dp(7));
        GradientDrawable background = new GradientDrawable();
        background.setColor(Color.argb(205, 3, 11, 16));
        background.setCornerRadius(dp(8));
        background.setStroke(dp(1), Color.rgb(57, 167, 192));
        view.setBackground(background);
        view.setClickable(false);
        view.setFocusable(false);
        view.setElevation(dp(20));
        RelativeLayout.LayoutParams params = new RelativeLayout.LayoutParams(
                RelativeLayout.LayoutParams.WRAP_CONTENT,
                RelativeLayout.LayoutParams.WRAP_CONTENT);
        params.addRule(RelativeLayout.ALIGN_PARENT_TOP);
        params.addRule(RelativeLayout.ALIGN_PARENT_RIGHT);
        params.topMargin = dp(14);
        params.rightMargin = dp(16);
        mLayout.addView(view, params);
        view.bringToFront();
        fpsOverlay = view;
    }

    private void startFpsUpdates() {
        if (fpsHandler == null) {
            fpsHandler = new Handler(Looper.getMainLooper());
        }
        fpsHandler.removeCallbacks(fpsUpdateRunnable);
        fpsHandler.post(fpsUpdateRunnable);
    }

    private void stopFpsUpdates() {
        if (fpsHandler != null) {
            fpsHandler.removeCallbacks(fpsUpdateRunnable);
        }
    }

    private void updateFpsOverlay() {
        try {
            long frames = nativeGetFrameCounter();
            long packed = nativeGetFrameStats();
            double average = (packed & 0xffffL) / 10.0;
            double onePercentLow = ((packed >>> 16) & 0xffffL) / 10.0;
            double minimum = ((packed >>> 32) & 0xffffL) / 10.0;
            if (average > 0.0) {
                double frameMs = 1000.0 / average;
                if (fpsOverlay != null) {
                    fpsOverlay.setText(String.format(Locale.US,
                            "FPS %.1f  1%% low %.1f" + "\n"
                                    + "min %.1f  %.1f ms  #%d",
                            average, onePercentLow, minimum, frameMs, frames));
                }
                if (logStats && (++statsLogTick % 4) == 0) {
                    Log.i(TAG, String.format(Locale.US,
                            "[DSstats] fps=%.1f low=%.1f min=%.1f frame_ms=%.2f frames=%d",
                            average, onePercentLow, minimum, frameMs, frames));
                }
            } else if (fpsOverlay != null) {
                fpsOverlay.setText(String.format(Locale.US, "FPS --" + "\n" + "frame %d", frames));
            }
        } catch (Throwable t) {
            if (fpsOverlay != null) {
                fpsOverlay.setText("FPS --" + "\n" + "waiting for renderer");
            }
        }
        if (fpsHandler != null) {
            fpsHandler.postDelayed(fpsUpdateRunnable, 500);
        }
    }

    private int dp(int value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
    }

    private static native long nativeGetFrameCounter();
    private static native long nativeGetFrameStats();

    private File ensureBundledIpaCopied() throws IOException {
        AssetManager assets = getAssets();
        String bundleId = readAssetText(assets, BUNDLED_IPA_ID_ASSET).trim();
        if (bundleId.isEmpty()) {
            throw new IOException("Bundled IPA id asset is empty");
        }

        File base = getExternalFilesDir(null);
        if (base == null) {
            base = getFilesDir();
        }
        File appsDirectory = new File(base, "touchHLE_apps");
        if (!appsDirectory.isDirectory() && !appsDirectory.mkdirs()) {
            throw new IOException("Could not create " + appsDirectory);
        }

        File destination = new File(appsDirectory, BUNDLED_IPA_FILENAME);
        File idFile = new File(appsDirectory, "." + BUNDLED_IPA_FILENAME + ".bundle-id");
        if (destination.isFile() && idFile.isFile()
                && bundleId.equals(readFileText(idFile).trim())) {
            return destination;
        }

        File temporary = new File(appsDirectory, "." + BUNDLED_IPA_FILENAME + ".part");
        runOnUiThread(() -> Toast.makeText(
                this,
                "Preparing bundled game data. First launch can take a while.",
                Toast.LENGTH_LONG
        ).show());
        try (InputStream input = assets.open(BUNDLED_IPA_ASSET);
             FileOutputStream output = new FileOutputStream(temporary)) {
            byte[] buffer = new byte[1024 * 1024];
            int count;
            while ((count = input.read(buffer)) != -1) {
                output.write(buffer, 0, count);
            }
            output.getFD().sync();
        }

        if (destination.exists() && !destination.delete()) {
            throw new IOException("Could not replace " + destination);
        }
        if (!temporary.renameTo(destination)) {
            throw new IOException("Could not install " + destination);
        }
        try (FileOutputStream output = new FileOutputStream(idFile)) {
            output.write(bundleId.getBytes(StandardCharsets.UTF_8));
            output.getFD().sync();
        }
        return destination;
    }

    private static String readAssetText(AssetManager assets, String path) throws IOException {
        try (InputStream input = assets.open(path)) {
            byte[] bytes = new byte[4096];
            int count = input.read(bytes);
            if (count < 0) {
                return "";
            }
            return new String(bytes, 0, count, StandardCharsets.UTF_8);
        }
    }

    private static String readFileText(File file) throws IOException {
        try (FileInputStream input = new FileInputStream(file)) {
            byte[] bytes = new byte[4096];
            int count = input.read(bytes);
            if (count < 0) {
                return "";
            }
            return new String(bytes, 0, count, StandardCharsets.UTF_8);
        }
    }
}
