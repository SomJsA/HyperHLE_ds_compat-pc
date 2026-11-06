package org.touchhle.android;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.content.SharedPreferences;
import android.net.Uri;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.os.Build;
import android.os.Bundle;
import android.view.Gravity;
import android.view.View;
import android.view.Window;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.view.WindowManager;
import android.widget.Button;
import android.widget.CheckBox;
import android.widget.FrameLayout;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;
import android.widget.Toast;

import java.io.File;

/**
 * Dead Space launch settings. Kept separate from SDLActivity so native audio /
 * GL startup does not begin until the player has chosen optional diagnostics.
 */
public class LauncherActivity extends Activity {
    static final String PREFS = "dead_space_launch";
    static final String KEY_FPS = "show_fps";
    static final String KEY_LOGS = "debug_logs";
    static final String KEY_DUMPS = "dump_frames";
    static final String KEY_COMPAT = "pc_compat";
    static final String KEY_SCALE = "scale_hack";
    static final String KEY_CAP = "fps_cap";
    static final String KEY_STRETCH = "viewport_stretch";
    static final String KEY_FAST_PRESENT = "fast_present";
    static final String KEY_SKIP_READBACK = "skip_readback";
    static final String KEY_GPU_COPY = "gpu_present_copy";
    static final String KEY_FSR = "present_fsr";
    static final String KEY_AA = "aa_mode";
    static final String KEY_SHARPEN = "present_sharpen";
    static final String KEY_FSR3_AHB = "fsr3_ahb_probe";
    static final String KEY_FSR3_FG = "fsr3_frame_generation";
    static final String KEY_FSR3_DEPTH = "fsr3_real_depth";
    static final String KEY_FSR3_GPU_DEPTH = "fsr3_gpu_depth";
    static final String KEY_FSR3_ASYNC = "fsr3_async";
    static final String KEY_FSR3_DIRECT_OUTPUT = "fsr3_direct_output";
    static final String KEY_FSR3_FORCE = "fsr3_force_unsupported";
    static final String KEY_PRESET = "quality_preset";

    static final String PRESET_RECOMMENDED = "recommended";
    static final String PRESET_PERFORMANCE = "performance";
    static final String PRESET_COMPATIBILITY = "compatibility";
    static final String PRESET_FSR3 = "fsr3_experimental";
    static final String PRESET_CUSTOM = "custom";

    private static final String[] PRESET_KEYS = {
            PRESET_RECOMMENDED,
            PRESET_PERFORMANCE,
            PRESET_COMPATIBILITY,
            PRESET_FSR3,
            PRESET_CUSTOM
    };
    private static final String[] PRESET_LABELS = {
            "推荐画质",
            "性能优先",
            "兼容优先",
            "FSR3 实验",
            "自定义"
    };

    private static final int COLOR_BG = Color.rgb(7, 11, 17);
    private static final int COLOR_PANEL = Color.rgb(12, 21, 30);
    private static final int COLOR_PANEL_ALT = Color.rgb(17, 32, 43);
    private static final int COLOR_LINE = Color.rgb(39, 65, 79);
    private static final int COLOR_ACCENT = Color.rgb(102, 224, 232);
    private static final int COLOR_TEXT = Color.rgb(235, 247, 249);
    private static final int COLOR_MUTED = Color.rgb(132, 158, 168);
    private static final int COLOR_WARN = Color.rgb(238, 168, 91);
    private static final int REQUEST_IMPORT_SAVE_ZIP = 41;

    private SharedPreferences prefs;
    private CheckBox fpsToggle;
    private CheckBox logsToggle;
    private CheckBox dumpsToggle;
    private CheckBox compatToggle;
    private CheckBox fastPresentToggle;
    private CheckBox skipReadbackToggle;
    private CheckBox gpuCopyToggle;
    private CheckBox fsr3AhbToggle;
    private CheckBox fsr3ForceToggle;
    private CheckBox fsr3FgToggle;
    private CheckBox fsr3DepthToggle;
    private CheckBox fsr3GpuDepthToggle;
    private CheckBox fsr3AsyncToggle;
    private CheckBox fsr3DirectOutputToggle;
    private TextView fsr3SelectionStatus;
    private TextView presetSummary;
    private TextView saveImportStatus;
    private Button advancedButton;
    private LinearLayout advancedContainer;
    private final Button[] presetButtons = new Button[PRESET_KEYS.length];
    private boolean fsr3Supported;
    private Button scaleButton;
    private Button capButton;
    private Button stretchButton;
    private Button fsrButton;
    private Button aaButton;
    private Button sharpenButton;
    private int scaleHack;
    private int fpsCap;
    private boolean stretchViewport;
    private int fsrMode;
    private boolean aaEnabled;
    private int sharpenTenths;
    private String qualityPreset;
    private boolean advancedExpanded;
    private boolean applyingPreset;
    private boolean showFps;
    private boolean debugLogs;
    private boolean dumpFrames;
    private boolean pcCompat;
    private boolean fastPresent;
    private boolean skipReadback;
    private boolean gpuPresentCopy;
    private boolean fsr3AhbEnabled;
    private boolean fsr3ForceEnabled;
    private boolean fsr3FgEnabled;
    private boolean fsr3DepthEnabled;
    private boolean fsr3GpuDepthEnabled;
    private boolean fsr3AsyncEnabled;
    private boolean fsr3DirectOutputEnabled;

    @Override
    public void onBackPressed() {
        new AlertDialog.Builder(this)
                .setTitle("离开启动器？")
                .setMessage("当前没有运行中的游戏。返回桌面后，游戏不会自行退出。")
                .setPositiveButton("返回桌面", (d, which) -> finish())
                .setNegativeButton("取消", null)
                .show();
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        prefs = getSharedPreferences(PREFS, MODE_PRIVATE);
        fsr3Supported = Fsr3Capabilities.supportsBasicVulkanPath(this);
        boolean freshInstall = prefs.getAll().isEmpty();
        scaleHack = prefs.getInt(KEY_SCALE, 2);
        fpsCap = prefs.getInt(KEY_CAP, 0);
        stretchViewport = prefs.getBoolean(KEY_STRETCH, false);
        fsrMode = prefs.getInt(KEY_FSR, 0);
        aaEnabled = prefs.getBoolean(KEY_AA, false);
        sharpenTenths = prefs.getInt(KEY_SHARPEN, 0);
        showFps = prefs.getBoolean(KEY_FPS, true);
        debugLogs = prefs.getBoolean(KEY_LOGS, false);
        dumpFrames = prefs.getBoolean(KEY_DUMPS, false);
        pcCompat = prefs.getBoolean(KEY_COMPAT, true);
        fastPresent = prefs.getBoolean(KEY_FAST_PRESENT, true);
        skipReadback = prefs.getBoolean(KEY_SKIP_READBACK, true);
        gpuPresentCopy = prefs.getBoolean(KEY_GPU_COPY, true);
        fsr3AhbEnabled = prefs.getBoolean(KEY_FSR3_AHB, false);
        fsr3ForceEnabled = prefs.getBoolean(KEY_FSR3_FORCE, false);
        fsr3FgEnabled = false;
        fsr3DepthEnabled = prefs.getBoolean(KEY_FSR3_DEPTH, false);
        fsr3GpuDepthEnabled = prefs.getBoolean(KEY_FSR3_GPU_DEPTH, false);
        fsr3AsyncEnabled = prefs.getBoolean(KEY_FSR3_ASYNC, false);
        fsr3DirectOutputEnabled = prefs.getBoolean(KEY_FSR3_DIRECT_OUTPUT, false);

        if (freshInstall) {
            qualityPreset = PRESET_RECOMMENDED;
            applyPresetValues(PRESET_RECOMMENDED);
            saveCurrentSettings();
        } else {
            qualityPreset = prefs.getString(KEY_PRESET, "");
            if (qualityPreset == null || qualityPreset.isEmpty()) {
                qualityPreset = detectCurrentPreset();
            }
            if (PRESET_FSR3.equals(qualityPreset) && !fsr3Usable()) {
                qualityPreset = PRESET_COMPATIBILITY;
                applyPresetValues(PRESET_COMPATIBILITY);
                saveCurrentSettings();
            }
        }

        // Automated real-device runs launch the exported launcher directly with
        // explicit extras. This avoids screenshot-driven UI control while still
        // exercising the same MainActivity startup path as an interactive run.
        Intent launchIntent = getIntent();
        if (launchIntent.getBooleanExtra("autorun", false)) {
            qualityPreset = PRESET_CUSTOM;
            startGameWithConfig(
                    launchIntent.getBooleanExtra("show_fps", false),
                    launchIntent.getBooleanExtra("debug_logs", false),
                    launchIntent.getBooleanExtra("dump_frames", false),
                    launchIntent.getBooleanExtra("pc_compat", true),
                    launchIntent.getBooleanExtra("fast_present", true),
                    launchIntent.getBooleanExtra("skip_readback", true),
                    launchIntent.getBooleanExtra("gpu_present_copy", false),
                    Math.max(1, Math.min(3, launchIntent.getIntExtra("scale_hack", 2))),
                    launchIntent.getIntExtra("fps_cap", 0),
                    launchIntent.getBooleanExtra("viewport_stretch", false),
                    launchIntent.getIntExtra("present_fsr", 0),
                    launchIntent.getBooleanExtra("aa", false),
                    launchIntent.getIntExtra("present_sharpen", 0),
                    launchIntent.getBooleanExtra("stats_log", false),
                    launchIntent.getBooleanExtra("fsr3_ahb_probe", false),
                    launchIntent.getBooleanExtra("fsr3_fg", false),
                    launchIntent.getBooleanExtra("fsr3_real_depth", false),
                    launchIntent.getBooleanExtra("fsr3_gpu_depth", false),
                    launchIntent.getBooleanExtra("fsr3_async", false),
                    launchIntent.getBooleanExtra("fsr3_direct_output", false),
                    launchIntent.getBooleanExtra("fsr3_diag_readback", false),
                    launchIntent.getBooleanExtra("fsr3_trace", false),
                    launchIntent.getBooleanExtra("fsr3_fg_synthetic", false),
                    launchIntent.getBooleanExtra("fsr3_fg_probe", false),
                    launchIntent.getStringExtra("ipa_path"));
            return;
        }

        setContentView(buildUi());
        applyImmersiveMode();
        getWindow().getDecorView().post(this::applyImmersiveMode);
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode != REQUEST_IMPORT_SAVE_ZIP) {
            return;
        }
        if (resultCode != RESULT_OK || data == null || data.getData() == null) {
            updateSaveImportStatus("已取消 ZIP 导入。", COLOR_MUTED);
            return;
        }
        importSaveZip(data.getData());
    }

    private void chooseSaveZip() {
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        // Some Android file providers classify ZIPs as octet-stream or expose
        // no MIME type at all. Accept any selectable file and validate the ZIP
        // plus its .sb contents in SaveArchiveImporter.
        intent.setType("*/*");
        try {
            startActivityForResult(intent, REQUEST_IMPORT_SAVE_ZIP);
        } catch (ActivityNotFoundException error) {
            updateSaveImportStatus("当前设备没有可用的文件选择器。", COLOR_WARN);
        }
    }

    private void importSaveZip(Uri uri) {
        updateSaveImportStatus("正在校验并导入 ZIP，请勿关闭启动器……", COLOR_ACCENT);
        new Thread(() -> {
            try {
                SaveArchiveCore.Result result = SaveArchiveImporter.importZip(this, uri);
                String message = "已导入 " + result.importedFiles + " 个 .sb 存档。";
                if (result.backup != null) {
                    message += "\n导入前备份：" + result.backup.getName();
                }
                String finalMessage = message;
                runOnUiThread(() -> updateSaveImportStatus(finalMessage, COLOR_TEXT));
            } catch (Exception error) {
                String message = "导入失败："
                        + (error.getMessage() == null ? error.getClass().getSimpleName()
                        : error.getMessage());
                runOnUiThread(() -> {
                    updateSaveImportStatus(message, COLOR_WARN);
                    Toast.makeText(this, message, Toast.LENGTH_LONG).show();
                });
            }
        }, "save-import").start();
    }

    private void updateSaveImportStatus(String message, int color) {
        if (saveImportStatus == null) {
            return;
        }
        saveImportStatus.setText(message);
        saveImportStatus.setTextColor(color);
    }

    private View buildUi() {
        ScrollView scroll = new ScrollView(this);
        scroll.setFillViewport(true);
        scroll.setBackgroundColor(COLOR_BG);

        LinearLayout content = new LinearLayout(this);
        content.setOrientation(LinearLayout.VERTICAL);
        content.setPadding(dp(28), dp(24), dp(28), dp(24));
        scroll.addView(content, new ScrollView.LayoutParams(
                ScrollView.LayoutParams.MATCH_PARENT,
                ScrollView.LayoutParams.WRAP_CONTENT));

        TextView eyebrow = text("DEAD SPACE  /  IOS PORT  /  1.3.34", 11, COLOR_ACCENT, true);
        eyebrow.setLetterSpacing(0.22f);
        eyebrow.setTypeface(Typeface.create(Typeface.MONOSPACE, Typeface.BOLD));
        content.addView(eyebrow);

        TextView title = text("死亡空间IOS移植版", 30, COLOR_TEXT, true);
        title.setPadding(0, dp(7), 0, dp(3));
        content.addView(title);

        TextView subtitle = text(
                "让我们把泰坦空间站烧成灰。检查画质与存档后开始游戏。",
                14, COLOR_MUTED, false);
        subtitle.setPadding(0, 0, 0, dp(16));
        content.addView(subtitle);

        FrameLayout hero = new FrameLayout(this);
        hero.setBackground(panelBackground(COLOR_PANEL, COLOR_LINE));
        LauncherBackdropView backdrop = new LauncherBackdropView(this);
        hero.addView(backdrop, new FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT,
                FrameLayout.LayoutParams.MATCH_PARENT));

        LinearLayout heroRow = new LinearLayout(this);
        heroRow.setOrientation(LinearLayout.HORIZONTAL);
        heroRow.setGravity(Gravity.CENTER_VERTICAL);
        heroRow.setPadding(dp(18), dp(16), dp(18), dp(16));

        ImageView gameIcon = new ImageView(this);
        gameIcon.setImageResource(R.drawable.icon);
        gameIcon.setScaleType(ImageView.ScaleType.CENTER_CROP);
        gameIcon.setBackground(panelBackground(COLOR_BG, COLOR_LINE));
        gameIcon.setClipToOutline(true);
        LinearLayout.LayoutParams iconParams = new LinearLayout.LayoutParams(dp(112), dp(112));
        iconParams.rightMargin = dp(18);
        heroRow.addView(gameIcon, iconParams);

        LinearLayout heroCopy = new LinearLayout(this);
        heroCopy.setOrientation(LinearLayout.VERTICAL);
        TextView heroKicker = text("ISHIMURA / USG / 02", 10, COLOR_ACCENT, true);
        heroKicker.setLetterSpacing(0.2f);
        heroKicker.setTypeface(Typeface.create(Typeface.MONOSPACE, Typeface.BOLD));
        heroCopy.addView(heroKicker);
        TextView heroTitle = text("DEAD SPACE", 27, COLOR_TEXT, true);
        heroTitle.setLetterSpacing(0.08f);
        heroTitle.setPadding(0, dp(2), 0, 0);
        heroCopy.addView(heroTitle);
        TextView heroName = text("死亡空间IOS移植版", 17, COLOR_TEXT, true);
        heroCopy.addView(heroName);
        TextView heroBody = text(
                "原版 1.3.34 内容 · PC 主线修复 · Android 触摸与横屏适配",
                12, COLOR_MUTED, false);
        heroBody.setPadding(0, dp(5), 0, dp(8));
        heroCopy.addView(heroBody);
        LinearLayout chips = new LinearLayout(this);
        chips.setOrientation(LinearLayout.HORIZONTAL);
        chips.addView(chip("原生 3×"));
        chips.addView(chip("FXAA + CAS"));
        chips.addView(chip("PC FIX"));
        heroCopy.addView(chips);
        heroRow.addView(heroCopy, new LinearLayout.LayoutParams(
                0, LinearLayout.LayoutParams.WRAP_CONTENT, 1.0f));

        LinearLayout credits = new LinearLayout(this);
        credits.setOrientation(LinearLayout.VERTICAL);
        credits.setGravity(Gravity.END);
        credits.setPadding(dp(16), 0, 0, 0);
        TextView creditsLabel = text("CREW / CREDITS", 9, COLOR_ACCENT, true);
        creditsLabel.setLetterSpacing(0.18f);
        creditsLabel.setTypeface(Typeface.create(Typeface.MONOSPACE, Typeface.BOLD));
        creditsLabel.setGravity(Gravity.END);
        credits.addView(creditsLabel);
        TextView keyboardCredit = text("键鼠测试：天雷本雷", 11, COLOR_TEXT, false);
        keyboardCredit.setGravity(Gravity.END);
        keyboardCredit.setPadding(0, dp(5), 0, 0);
        credits.addView(keyboardCredit);
        TextView remainingCredit = text("剩余全部工作：SomJs", 11, COLOR_MUTED, false);
        remainingCredit.setGravity(Gravity.END);
        remainingCredit.setPadding(0, dp(2), 0, 0);
        credits.addView(remainingCredit);
        heroRow.addView(credits, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT,
                LinearLayout.LayoutParams.WRAP_CONTENT));
        hero.addView(heroRow, new FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT,
                FrameLayout.LayoutParams.WRAP_CONTENT));
        content.addView(hero, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT));

        LinearLayout qualityBlock = new LinearLayout(this);
        qualityBlock.setOrientation(LinearLayout.VERTICAL);
        content.addView(qualityBlock, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT));

        addSection(qualityBlock, "02  /  画质模式");
        TextView presetHint = text(
                "推荐：原生 3× + FXAA + CAS 0.5，FSR3 关闭。"
                        + "先选常用预设；需要逐项调整时再进入自定义。",
                13, COLOR_MUTED, false);
        presetHint.setPadding(0, dp(4), 0, dp(8));
        qualityBlock.addView(presetHint);
        addPresetRow(qualityBlock, 0, 2);
        addPresetRow(qualityBlock, 2, 2);
        addPresetRow(qualityBlock, 4, 1);

        presetSummary = text("", 14, COLOR_TEXT, false);
        presetSummary.setLineSpacing(0f, 1.08f);
        presetSummary.setPadding(dp(16), dp(12), dp(16), dp(12));
        presetSummary.setBackground(
                panelBackground(COLOR_PANEL_ALT, COLOR_LINE));
        LinearLayout.LayoutParams summaryParams = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT);
        summaryParams.topMargin = dp(2);
        summaryParams.bottomMargin = dp(8);
        qualityBlock.addView(presetSummary, summaryParams);

        advancedButton = button("高级参数：展开（自定义）   ›", false);
        advancedButton.setOnClickListener(v -> {
            advancedExpanded = !advancedExpanded;
            updateAdvancedVisibility();
        });
        LinearLayout.LayoutParams advancedButtonParams = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, dp(54));
        advancedButtonParams.bottomMargin = dp(6);
        qualityBlock.addView(advancedButton, advancedButtonParams);

        advancedContainer = new LinearLayout(this);
        advancedContainer.setOrientation(LinearLayout.VERTICAL);
        advancedExpanded = PRESET_CUSTOM.equals(qualityPreset);
        advancedContainer.setVisibility(View.GONE);
        qualityBlock.addView(advancedContainer, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT));

        addSection(advancedContainer, "FSR3 / Vulkan");
        TextView fsr3Status = text(Fsr3Capabilities.summary(this), 13, COLOR_MUTED, false);
        fsr3Status.setPadding(0, dp(4), 0, dp(8));
        advancedContainer.addView(fsr3Status);
        addInfoCard(advancedContainer,
                "当前是什么状态",
                "这是带真实 D16 深度、异步一帧流水线的 FSR3.1 实验路径，仍不是完整时间超分。\n"
                        + "已接通：RGBA8 颜色、游戏深度、AHB/EGLImage、Vulkan 外部内存和超分输出。\n"
                        + "仍缺失：真实运动向量、相机 jitter、有效历史帧和 HUD/UI 分层。\n"
                        + "异步模式会固定增加约 1 帧显示延迟；它不是帧生成，也不能对外宣称完整 FSR3。",
                COLOR_WARN);
        addInfoCard(advancedContainer,
                "Adreno 740 实机基线与优化结果",
                "固定第 8 章、2376x1080 Surface、录像后段中位数：\n"
                        + "原生 960x1440：60.2 FPS / 16.56 ms / 1% low 41.8。\n"
                        + "同步 FSR 640x960：21.1 FPS / 49.26 ms / 1% low 5.7。\n"
                        + "异步 FSR 640x960：50.5 FPS / 19.84 ms / 1% low 38.1。\n"
                        + "异步 FSR 960x1440：44.8 FPS / 22.17 ms / 1% low 35.4。\n"
                        + "PSS 约 2.8 GB；直写 2× 曾异常到 4.37 GB。"
                        + "仅 Adreno 740 有这组证据，未知 GPU 必须回退原生。",
                COLOR_WARN);
        addInfoCard(advancedContainer,
                "兼容门槛",
                "要求 Android 12+、arm64-v8a、Vulkan 1.2+ Compute、AHB 外部内存、"
                        + "EGL 外部图像和 EGL_KHR_gl_renderbuffer_image。\n"
                        + "当前只有 Adreno 740 有真实深度和 gameplay 证据；未知 GPU 应回退原生。\n"
                        + "游戏固定横屏，用户旋转不属于验收需求。",
                COLOR_ACCENT);
        fsr3ForceToggle = addToggle(advancedContainer,
                "强制启用 FSR3（高风险）",
                "即使 Vulkan 1.2 / AHB / EGLImage 预检失败也把开关传给原生层。"
                        + "用于收集驱动实测证据，不保证可运行；出现黑屏或闪退请关闭。",
                fsr3ForceEnabled);
        fsr3AhbToggle = addToggle(advancedContainer,
                "FSR3.1 Vulkan 超分（实验）",
                "总开关。把游戏 GLES 颜色通过共享 AHB 送入 FSR3.1 Vulkan。"
                        + "关闭后立即回到原生呈现；只有同时开启下面的真实深度，"
                        + "FSR 才会收到 depth=true。",
                fsr3AhbEnabled);
        fsr3FgToggle = addToggle(advancedContainer,
                "FSR3 帧生成（暂不可用）",
                "没有真实 per-pixel motion、低延迟呈现和 UI 分层。"
                        + "普通开关会安全拒绝，不能用它冒充帧数提升。",
                false);
        fsr3DepthToggle = addToggle(advancedContainer,
                "真实深度输入 D16（实验）",
                "通过 EGLImage 读取游戏深度并上传给 FSR3。"
                        + "该路径每帧有 CPU 读回开销，但缺少它时 depth=false，"
                        + "只能验证呈现链路，不能算完整超分输入。",
                fsr3DepthEnabled);
        fsr3GpuDepthToggle = addToggle(advancedContainer,
                "GPU 深度直通（实验）",
                "通过 EGLImage 和 glBlitFramebuffer 将 D16 深度写入 AHardwareBuffer，"
                        + "避免每帧 CPU glReadPixels。仅 Adreno 740 已验证；失败时回退 CPU。",
                fsr3GpuDepthEnabled);
        fsr3AsyncToggle = addToggle(advancedContainer,
                "异步 FSR 流水线（实验）",
                "在 CPU 侧不等当前 FSR GPU 完成：提交当前帧后显示上一帧已完成的 FSR 输出，"
                        + "让当前游戏渲染与上一帧超分重叠。代价是固定增加约 1 帧显示延迟；"
                        + "首帧和异常时自动回退原生。默认关闭，帧生成/诊断读回开启时也不使用。",
                fsr3AsyncEnabled);
        fsr3DirectOutputToggle = addToggle(advancedContainer,
                "FSR 直接写输出 AHB（实验）",
                "让 FSR3 直接写入用于 GLES 呈现的 AHardwareBuffer，"
                        + "省掉 1620×1080 中间纹理到输出 AHB 的全画面复制。"
                        + "依赖异步流水线；驱动不支持外部存储图像时自动回退。默认关闭。",
                fsr3DirectOutputEnabled);
        fsr3SelectionStatus = text("", 13, COLOR_WARN, false);
        fsr3SelectionStatus.setLineSpacing(0f, 1.08f);
        fsr3SelectionStatus.setPadding(dp(16), dp(12), dp(16), dp(12));
        fsr3SelectionStatus.setBackground(
                panelBackground(COLOR_PANEL_ALT, COLOR_LINE));
        LinearLayout.LayoutParams statusParams = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT);
        statusParams.bottomMargin = dp(8);
        advancedContainer.addView(fsr3SelectionStatus, statusParams);
        updateFsr3ControlEnablement();
        fsr3FgToggle.setEnabled(false);
        fsr3ForceToggle.setOnCheckedChangeListener((button, checked) -> {
            fsr3ForceEnabled = checked;
            if (checked && !fsr3Supported) {
                Toast.makeText(this,
                        "已允许 FSR3 强开：此路径仅用于实验，可能黑屏或闪退。",
                        Toast.LENGTH_LONG).show();
            }
            if (!checked && !fsr3Supported) {
                fsr3AhbEnabled = false;
                setChecked(fsr3AhbToggle, false);
            }
            onQualitySettingChanged();
            updateFsr3SelectionStatus();
        });
        fsr3AhbToggle.setOnCheckedChangeListener((button, checked) -> {
            fsr3AhbEnabled = checked;
            onQualitySettingChanged();
            updateFsr3SelectionStatus();
        });
        fsr3DepthToggle.setOnCheckedChangeListener((button, checked) -> {
            fsr3DepthEnabled = checked;
            onQualitySettingChanged();
            updateFsr3SelectionStatus();
        });
        fsr3GpuDepthToggle.setOnCheckedChangeListener((button, checked) -> {
            fsr3GpuDepthEnabled = checked;
            onQualitySettingChanged();
            updateFsr3SelectionStatus();
        });
        fsr3AsyncToggle.setOnCheckedChangeListener((button, checked) -> {
            fsr3AsyncEnabled = checked;
            onQualitySettingChanged();
            updateFsr3SelectionStatus();
        });
        fsr3DirectOutputToggle.setOnCheckedChangeListener((button, checked) -> {
            fsr3DirectOutputEnabled = checked;
            onQualitySettingChanged();
            updateFsr3SelectionStatus();
        });
        updateFsr3SelectionStatus();

        addSection(advancedContainer, "性能");
        fpsToggle = addToggle(advancedContainer,
                "显示实际 FPS",
                "基于 presentRenderbuffer 的真实帧计数，显示 FPS、平均帧时间和累计帧号。",
                showFps);
        fpsToggle.setOnCheckedChangeListener((button, checked) -> {
            showFps = checked;
            saveCurrentSettings();
        });
        scaleButton = addCycleButton(advancedContainer, "", v -> {
            scaleHack = scaleHack >= 3 ? 1 : scaleHack + 1;
            updateChoiceLabels();
            onQualitySettingChanged();
        });
        capButton = addCycleButton(advancedContainer, "", v -> {
            fpsCap = fpsCap == 0 ? 60 : (fpsCap == 60 ? 30 : 0);
            updateChoiceLabels();
            onQualitySettingChanged();
        });
        stretchButton = addCycleButton(advancedContainer, "", v -> {
            stretchViewport = !stretchViewport;
            updateChoiceLabels();
            onQualitySettingChanged();
        });
        fastPresentToggle = addToggle(advancedContainer,
                "快速呈现",
                "绕过 glReadPixels 合成路径；当前 OnePlus 设备已验证稳定，若其他设备闪烁请关闭。",
                fastPresent);
        fastPresentToggle.setOnCheckedChangeListener((button, checked) -> {
            fastPresent = checked;
            onQualitySettingChanged();
        });
        skipReadbackToggle = addToggle(advancedContainer,
                "跳过读回 Finish",
                "省掉每帧 glFinish；当前设备已验证画面正常，若出现黑帧请关闭。",
                skipReadback);
        skipReadbackToggle.setOnCheckedChangeListener((button, checked) -> {
            skipReadback = checked;
            onQualitySettingChanged();
        });
        gpuCopyToggle = addToggle(advancedContainer,
                "GPU 复制呈现（实验）",
                "用 GPU 复制 renderbuffer 代替每帧 CPU 读回；失败时关闭并回退。",
                gpuPresentCopy);
        gpuCopyToggle.setOnCheckedChangeListener((button, checked) -> {
            gpuPresentCopy = checked;
            onQualitySettingChanged();
        });

        addSection(advancedContainer, "FSR / 抗锯齿");
        fsrButton = addCycleButton(advancedContainer, "", v -> {
            fsrMode = fsrMode >= 2 ? 0 : fsrMode + 1;
            updateChoiceLabels();
            onQualitySettingChanged();
        });
        aaButton = addCycleButton(advancedContainer, "", v -> {
            aaEnabled = !aaEnabled;
            updateChoiceLabels();
            onQualitySettingChanged();
        });
        sharpenButton = addCycleButton(advancedContainer, "", v -> {
            sharpenTenths = sharpenTenths == 0 ? 5 : (sharpenTenths == 5 ? 10 : 0);
            updateChoiceLabels();
            onQualitySettingChanged();
        });
        TextView qualityHint = text(
                "FSR、FXAA 与 CAS 会提升清晰度，但会增加 GPU 负载；速度优先时请保持关闭。",
                13, COLOR_MUTED, false);
        qualityHint.setPadding(0, dp(4), 0, dp(8));
        advancedContainer.addView(qualityHint);

        addSection(advancedContainer, "诊断");
        logsToggle = addToggle(advancedContainer,
                "详细调试日志",
                "开启投影、雾、draw、纹理和 FPS 日志；会增加 CPU 与 logcat 负载。",
                debugLogs);
        logsToggle.setOnCheckedChangeListener((button, checked) -> {
            debugLogs = checked;
            saveCurrentSettings();
        });
        dumpsToggle = addToggle(advancedContainer,
                "帧转储",
                "每 600 帧写入一张 PPM，文件较大且会明显影响帧率。默认关闭。",
                dumpFrames);
        dumpsToggle.setOnCheckedChangeListener((button, checked) -> {
            dumpFrames = checked;
            saveCurrentSettings();
        });

        addSection(advancedContainer, "兼容");
        compatToggle = addToggle(advancedContainer,
                "PC 渲染兼容修复",
                "投影 z、深度 LEQUAL、no-fog、ScreenScale=2 和字体缩放。建议保持开启。",
                pcCompat);
        compatToggle.setOnCheckedChangeListener((button, checked) -> {
            pcCompat = checked;
            onQualitySettingChanged();
        });

        TextView pathHint = text(
                "诊断文件目录：Android/data/org.touchhle.android/files/diagnostics",
                13, COLOR_MUTED, false);
        pathHint.setPadding(0, dp(14), 0, dp(16));
        advancedContainer.addView(pathHint);

        LinearLayout saveBlock = new LinearLayout(this);
        saveBlock.setOrientation(LinearLayout.VERTICAL);
        content.addView(saveBlock, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT));
        addSection(saveBlock, "03  /  存档与文件");
        addInfoCard(
                saveBlock,
                "导入 ZIP 存档",
                "支持包含 Documents/var 的 ZIP，也支持直接包含 .sb 的存档包。"
                        + "导入前会先把当前 Documents 备份到 save-backups；"
                        + "请先关闭正在运行的游戏。",
                COLOR_ACCENT);
        Button importSave = button("选择 ZIP 存档并导入", false);
        importSave.setOnClickListener(v -> chooseSaveZip());
        LinearLayout.LayoutParams importParams = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, dp(50));
        importParams.bottomMargin = dp(8);
        saveBlock.addView(importSave, importParams);
        saveImportStatus = text(
                "尚未导入。仅接受 .sb 存档文件，ZIP 解压上限 256 MB。",
                12, COLOR_MUTED, false);
        saveImportStatus.setPadding(dp(14), dp(12), dp(14), dp(12));
        saveImportStatus.setBackground(panelBackground(COLOR_PANEL, COLOR_LINE));
        LinearLayout.LayoutParams saveStatusParams = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT);
        saveStatusParams.bottomMargin = dp(8);
        saveBlock.addView(saveImportStatus, saveStatusParams);
        addInfoCard(
                saveBlock,
                "存档编辑器  /  计划中",
                "当前版本不提供存档写入或字段修改。直接编辑 .sb 有损坏存档的风险"
                        + "（风险等级 1：可能导致存档无法读取）；"
                        + "后续实现仍会强制备份，而不是绕过校验。",
                COLOR_WARN);

        LinearLayout actions = new LinearLayout(this);
        actions.setOrientation(LinearLayout.HORIZONTAL);
        actions.setGravity(Gravity.CENTER_VERTICAL);

        Button reset = button("恢复默认", false);
        reset.setOnClickListener(v -> resetDefaults());
        actions.addView(reset, new LinearLayout.LayoutParams(0, dp(54), 1.0f));

        View gap = new View(this);
        actions.addView(gap, new LinearLayout.LayoutParams(dp(12), dp(1)));

        Button start = button("启动游戏", true);
        start.setOnClickListener(v -> startGame());
        actions.addView(start, new LinearLayout.LayoutParams(0, dp(54), 1.7f));
        updateChoiceLabels();
        updatePresetUi();
        updateAdvancedVisibility();

        LinearLayout page = new LinearLayout(this);
        page.setOrientation(LinearLayout.HORIZONTAL);
        page.setBackgroundColor(COLOR_BG);
        View rail = buildNavigationRail(scroll, hero, qualityBlock, advancedContainer, saveBlock);
        page.addView(rail, new LinearLayout.LayoutParams(dp(190),
                LinearLayout.LayoutParams.MATCH_PARENT));

        LinearLayout main = new LinearLayout(this);
        main.setOrientation(LinearLayout.VERTICAL);
        main.setBackgroundColor(COLOR_BG);
        main.addView(scroll, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, 0, 1.0f));
        actions.setPadding(dp(28), dp(8), dp(28), dp(20));
        main.addView(actions, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT));
        page.addView(main, new LinearLayout.LayoutParams(
                0, LinearLayout.LayoutParams.MATCH_PARENT, 1.0f));
        return page;
    }

    private View buildNavigationRail(
            ScrollView scroll, View overview, View quality, View advanced, View saves) {
        LinearLayout rail = new LinearLayout(this);
        rail.setOrientation(LinearLayout.VERTICAL);
        rail.setPadding(dp(18), dp(24), dp(18), dp(18));
        rail.setBackground(panelBackground(COLOR_BG, COLOR_LINE));

        TextView brand = text("DEAD SPACE", 17, COLOR_TEXT, true);
        brand.setLetterSpacing(0.24f);
        brand.setTypeface(Typeface.create(Typeface.MONOSPACE, Typeface.BOLD));
        rail.addView(brand);
        TextView brandSub = text("IOS PORT  /  1.3.34", 10, COLOR_ACCENT, true);
        brandSub.setLetterSpacing(0.18f);
        brandSub.setTypeface(Typeface.create(Typeface.MONOSPACE, Typeface.BOLD));
        brandSub.setPadding(0, dp(2), 0, dp(5));
        rail.addView(brandSub);
        TextView brandName = text("死亡空间IOS移植版", 13, COLOR_MUTED, false);
        brandName.setPadding(0, 0, 0, dp(22));
        rail.addView(brandName);

        rail.addView(railButton("01   概览", () -> scrollToSection(scroll, overview)));
        rail.addView(railButton("02   画质", () -> scrollToSection(scroll, quality)));
        rail.addView(railButton("03   实验参数", () -> {
            advancedExpanded = true;
            updateAdvancedVisibility();
            scrollToSection(scroll, advanced);
        }));
        rail.addView(railButton("04   存档与文件", () -> scrollToSection(scroll, saves)));

        View spacer = new View(this);
        rail.addView(spacer, new LinearLayout.LayoutParams(1, 0, 1.0f));
        TextView footer = text(
                "PC CONTENT AUTHORITY\nANDROID NATIVE PATH\n\n"
                        + BuildConfig.VERSION_NAME,
                10, COLOR_MUTED, false);
        footer.setTypeface(Typeface.create(Typeface.MONOSPACE, Typeface.NORMAL));
        footer.setLetterSpacing(0.08f);
        footer.setPadding(0, dp(12), 0, 0);
        rail.addView(footer);
        return rail;
    }

    private Button railButton(String label, Runnable action) {
        Button button = button(label, false);
        button.setGravity(Gravity.START | Gravity.CENTER_VERTICAL);
        button.setTypeface(Typeface.create(Typeface.MONOSPACE, Typeface.NORMAL));
        button.setTextSize(12);
        button.setOnClickListener(v -> action.run());
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, dp(44));
        params.bottomMargin = dp(6);
        button.setLayoutParams(params);
        return button;
    }

    private void scrollToSection(ScrollView scroll, View target) {
        if (target == null) {
            return;
        }
        target.post(() -> {
            int[] targetLocation = new int[2];
            int[] scrollLocation = new int[2];
            target.getLocationInWindow(targetLocation);
            scroll.getLocationInWindow(scrollLocation);
            int y = scroll.getScrollY() + targetLocation[1] - scrollLocation[1] - dp(10);
            scroll.smoothScrollTo(0, Math.max(0, y));
        });
    }

    private TextView chip(String label) {
        TextView chip = text(label, 10, COLOR_ACCENT, true);
        chip.setTypeface(Typeface.create(Typeface.MONOSPACE, Typeface.BOLD));
        chip.setLetterSpacing(0.08f);
        chip.setPadding(dp(8), dp(4), dp(8), dp(4));
        chip.setBackground(panelBackground(COLOR_PANEL_ALT, COLOR_LINE));
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT,
                LinearLayout.LayoutParams.WRAP_CONTENT);
        params.rightMargin = dp(6);
        chip.setLayoutParams(params);
        return chip;
    }

    private void updateChoiceLabels() {
        String scaleText;
        if (scaleHack <= 1) {
            scaleText = "内部渲染：1× 性能";
        } else if (scaleHack == 2) {
            scaleText = "内部渲染：2× 平衡";
        } else {
            scaleText = "内部渲染：3× 清晰";
        }
        scaleButton.setText(scaleText + "   ›");

        String capText;
        if (fpsCap == 30) {
            capText = "帧率上限：30 FPS";
        } else if (fpsCap == 60) {
            capText = "帧率上限：60 FPS（模拟垂直同步）";
        } else {
            capText = "帧率上限：不限制（推荐）";
        }
        capButton.setText(capText + "   ›");
        stretchButton.setText((stretchViewport
                ? "画面比例：拉伸铺满（会变形）"
                : "画面比例：保持 3:2") + "   ›");

        String fsrText;
        if (fsrMode == 1) {
            fsrText = "FSR：EASU + RCAS";
        } else if (fsrMode == 2) {
            fsrText = "FSR：EASU 快速";
        } else {
            fsrText = "FSR：关闭";
        }
        fsrButton.setText(fsrText + "   ›");

        aaButton.setText((aaEnabled ? "抗锯齿：FXAA" : "抗锯齿：关闭") + "   ›");

        String sharpenText;
        if (sharpenTenths >= 10) {
            sharpenText = "CAS 锐化：1.0";
        } else if (sharpenTenths >= 5) {
            sharpenText = "CAS 锐化：0.5";
        } else {
            sharpenText = "CAS 锐化：关闭";
        }
        sharpenButton.setText(sharpenText + "   ›");
    }

    private void addPresetRow(LinearLayout root, int start, int count) {
        LinearLayout row = new LinearLayout(this);
        row.setOrientation(LinearLayout.HORIZONTAL);
        row.setGravity(Gravity.CENTER_VERTICAL);
        for (int index = start; index < start + count; index++) {
            final String preset = PRESET_KEYS[index];
            Button presetButton = button(PRESET_LABELS[index], false);
            presetButton.setOnClickListener(v -> applyPreset(preset, true));
            LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(
                    0, dp(54), 1.0f);
            if (index > start) {
                params.leftMargin = dp(8);
            }
            row.addView(presetButton, params);
            presetButtons[index] = presetButton;
        }
        LinearLayout.LayoutParams rowParams = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT);
        rowParams.bottomMargin = dp(8);
        root.addView(row, rowParams);
    }

    private void applyPreset(String preset, boolean userInitiated) {
        if (PRESET_FSR3.equals(preset) && !fsr3Usable()) {
            preset = PRESET_COMPATIBILITY;
        }
        applyingPreset = true;
        applyPresetValues(preset);
        syncPresetControls();
        applyingPreset = false;
        qualityPreset = preset;
        updateChoiceLabels();
        updatePresetUi();
        updateFsr3SelectionStatus();
        saveCurrentSettings();
        if (userInitiated) {
            advancedExpanded = PRESET_CUSTOM.equals(preset);
            updateAdvancedVisibility();
        }
    }

    private void applyPresetValues(String preset) {
        if (PRESET_RECOMMENDED.equals(preset)) {
            scaleHack = 3;
            fpsCap = 0;
            stretchViewport = false;
            fsrMode = 0;
            aaEnabled = true;
            sharpenTenths = 5;
            fastPresent = true;
            skipReadback = true;
            gpuPresentCopy = true;
            pcCompat = true;
            clearFsr3PresetValues();
        } else if (PRESET_PERFORMANCE.equals(preset)) {
            scaleHack = 1;
            fpsCap = 0;
            stretchViewport = false;
            fsrMode = 0;
            aaEnabled = false;
            sharpenTenths = 0;
            fastPresent = true;
            skipReadback = true;
            gpuPresentCopy = true;
            pcCompat = true;
            clearFsr3PresetValues();
        } else if (PRESET_COMPATIBILITY.equals(preset)) {
            scaleHack = 1;
            fpsCap = 0;
            stretchViewport = false;
            fsrMode = 0;
            aaEnabled = false;
            sharpenTenths = 0;
            fastPresent = false;
            skipReadback = false;
            gpuPresentCopy = false;
            pcCompat = true;
            clearFsr3PresetValues();
        } else if (PRESET_FSR3.equals(preset)) {
            scaleHack = 2;
            fpsCap = 0;
            stretchViewport = false;
            fsrMode = 0;
            aaEnabled = false;
            sharpenTenths = 0;
            fastPresent = true;
            skipReadback = true;
            gpuPresentCopy = true;
            pcCompat = true;
            fsr3AhbEnabled = true;
            fsr3FgEnabled = false;
            fsr3DepthEnabled = true;
            fsr3GpuDepthEnabled = true;
            fsr3AsyncEnabled = true;
            fsr3DirectOutputEnabled = false;
        }
    }

    private void clearFsr3PresetValues() {
        fsr3AhbEnabled = false;
        fsr3FgEnabled = false;
        fsr3DepthEnabled = false;
        fsr3GpuDepthEnabled = false;
        fsr3AsyncEnabled = false;
        fsr3DirectOutputEnabled = false;
    }

    private void syncPresetControls() {
        setChecked(fastPresentToggle, fastPresent);
        setChecked(skipReadbackToggle, skipReadback);
        setChecked(gpuCopyToggle, gpuPresentCopy);
        setChecked(compatToggle, pcCompat);
        setChecked(fsr3AhbToggle, fsr3AhbEnabled);
        setChecked(fsr3FgToggle, fsr3FgEnabled);
        setChecked(fsr3DepthToggle, fsr3DepthEnabled);
        setChecked(fsr3GpuDepthToggle, fsr3GpuDepthEnabled);
        setChecked(fsr3AsyncToggle, fsr3AsyncEnabled);
        setChecked(fsr3DirectOutputToggle, fsr3DirectOutputEnabled);
    }

    private void setChecked(CheckBox box, boolean checked) {
        if (box != null && box.isChecked() != checked) {
            box.setChecked(checked);
        }
    }

    private void onQualitySettingChanged() {
        if (applyingPreset) return;
        qualityPreset = PRESET_CUSTOM;
        saveCurrentSettings();
        updatePresetUi();
    }

    private void saveCurrentSettings() {
        prefs.edit()
                .putBoolean(KEY_FPS, showFps)
                .putBoolean(KEY_LOGS, debugLogs)
                .putBoolean(KEY_DUMPS, dumpFrames)
                .putBoolean(KEY_COMPAT, pcCompat)
                .putBoolean(KEY_FAST_PRESENT, fastPresent)
                .putBoolean(KEY_SKIP_READBACK, skipReadback)
                .putBoolean(KEY_GPU_COPY, gpuPresentCopy)
                .putInt(KEY_SCALE, scaleHack)
                .putInt(KEY_CAP, fpsCap)
                .putBoolean(KEY_STRETCH, stretchViewport)
                .putInt(KEY_FSR, fsrMode)
                .putBoolean(KEY_AA, aaEnabled)
                .putInt(KEY_SHARPEN, sharpenTenths)
                .putBoolean(KEY_FSR3_AHB, fsr3AhbEnabled)
                .putBoolean(KEY_FSR3_FG, fsr3FgEnabled)
                .putBoolean(KEY_FSR3_DEPTH, fsr3DepthEnabled)
                .putBoolean(KEY_FSR3_GPU_DEPTH, fsr3GpuDepthEnabled)
                .putBoolean(KEY_FSR3_ASYNC, fsr3AsyncEnabled)
                .putBoolean(KEY_FSR3_DIRECT_OUTPUT, fsr3DirectOutputEnabled)
                .putBoolean(KEY_FSR3_FORCE, fsr3ForceEnabled)
                .putString(KEY_PRESET, qualityPreset)
                .apply();
    }

    private String detectCurrentPreset() {
        if (matchesRecommendedPreset()) return PRESET_RECOMMENDED;
        if (matchesPerformancePreset()) return PRESET_PERFORMANCE;
        if (matchesCompatibilityPreset()) return PRESET_COMPATIBILITY;
        if (matchesFsr3Preset()) return PRESET_FSR3;
        return PRESET_CUSTOM;
    }

    private boolean matchesRecommendedPreset() {
        return scaleHack == 3
                && fpsCap == 0
                && !stretchViewport
                && fsrMode == 0
                && aaEnabled
                && sharpenTenths == 5
                && fastPresent
                && skipReadback
                && gpuPresentCopy
                && pcCompat
                && noFsr3Preset();
    }

    private boolean matchesPerformancePreset() {
        return scaleHack == 1
                && fpsCap == 0
                && !stretchViewport
                && fsrMode == 0
                && !aaEnabled
                && sharpenTenths == 0
                && fastPresent
                && skipReadback
                && gpuPresentCopy
                && pcCompat
                && noFsr3Preset();
    }

    private boolean matchesCompatibilityPreset() {
        return scaleHack == 1
                && fpsCap == 0
                && !stretchViewport
                && fsrMode == 0
                && !aaEnabled
                && sharpenTenths == 0
                && !fastPresent
                && !skipReadback
                && !gpuPresentCopy
                && pcCompat
                && noFsr3Preset();
    }

    private boolean matchesFsr3Preset() {
        return scaleHack == 2
                && fpsCap == 0
                && !stretchViewport
                && fsrMode == 0
                && !aaEnabled
                && sharpenTenths == 0
                && fastPresent
                && skipReadback
                && gpuPresentCopy
                && pcCompat
                && fsr3AhbEnabled
                && !fsr3FgEnabled
                && fsr3DepthEnabled
                && fsr3GpuDepthEnabled
                && fsr3AsyncEnabled
                && !fsr3DirectOutputEnabled;
    }

    private boolean noFsr3Preset() {
        return !fsr3AhbEnabled
                && !fsr3FgEnabled
                && !fsr3DepthEnabled
                && !fsr3GpuDepthEnabled
                && !fsr3AsyncEnabled
                && !fsr3DirectOutputEnabled;
    }

    private void updatePresetUi() {
        if (presetSummary == null) return;
        for (int index = 0; index < presetButtons.length; index++) {
            Button presetButton = presetButtons[index];
            if (presetButton == null) continue;
            boolean selected = PRESET_KEYS[index].equals(qualityPreset);
            presetButton.setTextColor(selected ? COLOR_BG : COLOR_TEXT);
            presetButton.setBackground(panelBackground(
                    selected ? COLOR_ACCENT : COLOR_PANEL_ALT,
                    selected ? Color.rgb(169, 245, 247) : COLOR_LINE));
            if (PRESET_FSR3.equals(PRESET_KEYS[index])) {
                presetButton.setEnabled(fsr3Usable());
                presetButton.setAlpha(fsr3Usable() ? 1.0f : 0.45f);
                if (!fsr3Usable()) {
                    presetButton.setText("FSR3 实验（不可用）");
                } else {
                    presetButton.setText(PRESET_LABELS[index]);
                }
            }
        }

        String scale = scaleHack + "×";
        String fxaa = aaEnabled ? "FXAA 开" : "FXAA 关";
        String cas = sharpenTenths == 0 ? "CAS 关"
                : String.format(java.util.Locale.US, "CAS %.1f", sharpenTenths / 10.0);
        if (PRESET_RECOMMENDED.equals(qualityPreset)) {
            presetSummary.setTextColor(COLOR_TEXT);
            presetSummary.setText(
                    "推荐配置总览\n"
                            + "原生 3× · FSR 关闭 · FXAA 开 · CAS 0.5 · 保持 3:2\n"
                            + "3× 基线约 60 FPS；FXAA/CAS 组合尚需分设备复测。");
            presetSummary.setBackground(
                    panelBackground(COLOR_PANEL_ALT, COLOR_LINE));
        } else if (PRESET_PERFORMANCE.equals(qualityPreset)) {
            presetSummary.setTextColor(COLOR_TEXT);
            presetSummary.setText(
                    "性能优先\n"
                            + "原生 1× · FSR/FXAA/CAS 关闭 · 快速呈现开启\n"
                            + "画质更软，优先保证帧时间和发热。");
            presetSummary.setBackground(
                    panelBackground(COLOR_PANEL_ALT, COLOR_LINE));
        } else if (PRESET_COMPATIBILITY.equals(qualityPreset)) {
            presetSummary.setTextColor(COLOR_TEXT);
            presetSummary.setText(
                    "兼容优先\n"
                            + "原生 1× · 关闭快速呈现/跳读回/GPU 复制 · PC 修复开启\n"
                            + "用于未知 GPU 或出现闪烁、黑帧的设备。");
            presetSummary.setBackground(
                    panelBackground(COLOR_PANEL_ALT, COLOR_LINE));
        } else if (PRESET_FSR3.equals(qualityPreset)) {
            presetSummary.setTextColor(COLOR_WARN);
            presetSummary.setText(
                    "FSR3 实验\n"
                            + "内部 2× · FSR3.1 + 真实 D16 + 异步一帧 · 直写关闭\n"
                            + "多约一帧延迟；缺 motion/jitter/history，不是完整时间超分或帧生成。");
            presetSummary.setBackground(
                    panelBackground(COLOR_PANEL, COLOR_WARN));
        } else {
            presetSummary.setTextColor(COLOR_TEXT);
            presetSummary.setText(
                    "自定义配置\n"
                            + "内部 " + scale + " · " + fxaa + " · " + cas
                            + " · " + (fsr3AhbEnabled ? "FSR3 开" : "FSR3 关")
                            + "\n展开高级参数可继续逐项调整。");
            presetSummary.setBackground(
                    panelBackground(COLOR_PANEL_ALT, COLOR_LINE));
        }
    }

    private void updateAdvancedVisibility() {
        if (advancedContainer == null || advancedButton == null) return;
        advancedContainer.setVisibility(advancedExpanded ? View.VISIBLE : View.GONE);
        advancedButton.setText(advancedExpanded
                ? "高级参数：收起   ›"
                : "高级参数：展开（自定义）   ›");
    }

    private void resetDefaults() {
        showFps = true;
        debugLogs = false;
        dumpFrames = false;
        setChecked(fpsToggle, showFps);
        setChecked(logsToggle, debugLogs);
        setChecked(dumpsToggle, dumpFrames);
        applyPreset(PRESET_RECOMMENDED, true);
    }

    private void startGame() {
        startGameWithConfig(
                showFps,
                debugLogs,
                dumpFrames,
                pcCompat,
                fastPresent,
                skipReadback,
                gpuPresentCopy,
                scaleHack,
                fpsCap,
                stretchViewport,
                fsrMode,
                aaEnabled,
                sharpenTenths,
                false,
                fsr3AhbEnabled,
                fsr3FgEnabled && fsr3AhbEnabled,
                fsr3DepthEnabled && fsr3AhbEnabled,
                fsr3GpuDepthEnabled && fsr3AhbEnabled && fsr3DepthEnabled,
                fsr3AsyncEnabled && fsr3AhbEnabled,
                fsr3DirectOutputEnabled && fsr3AhbEnabled && fsr3AsyncEnabled,
                false,
                false,
                false,
                false,
                null);
    }

    private void startGameWithConfig(
            boolean showFps,
            boolean debugLogs,
            boolean dumpFrames,
            boolean pcCompat,
            boolean fastPresent,
            boolean skipReadback,
            boolean gpuPresentCopy,
            int scale,
            int cap,
            boolean stretch,
            int fsr,
            boolean aa,
            int sharpen,
            boolean statsLog,
            boolean fsr3AhbProbe,
            boolean fsr3FrameGeneration,
            boolean fsr3RealDepth,
            boolean fsr3GpuDepth,
            boolean fsr3Async,
            boolean fsr3DirectOutput,
            boolean fsr3DiagReadback,
            boolean fsr3Trace,
            boolean fsr3FgSynthetic,
            boolean fsr3FgProbe,
            String ipaPath) {
        prefs.edit()
                .putBoolean(KEY_FPS, showFps)
                .putBoolean(KEY_LOGS, debugLogs)
                .putBoolean(KEY_DUMPS, dumpFrames)
                .putBoolean(KEY_COMPAT, pcCompat)
                .putBoolean(KEY_FAST_PRESENT, fastPresent)
                .putBoolean(KEY_SKIP_READBACK, skipReadback)
                .putBoolean(KEY_GPU_COPY, gpuPresentCopy)
                .putInt(KEY_SCALE, scale)
                .putInt(KEY_CAP, cap)
                .putBoolean(KEY_STRETCH, stretch)
                .putInt(KEY_FSR, fsr)
                .putBoolean(KEY_AA, aa)
                .putInt(KEY_SHARPEN, sharpen)
                .putBoolean(KEY_FSR3_AHB, fsr3AhbProbe)
                .putBoolean(KEY_FSR3_FG, fsr3FrameGeneration)
                .putBoolean(KEY_FSR3_DEPTH, fsr3RealDepth)
                .putBoolean(KEY_FSR3_GPU_DEPTH, fsr3GpuDepth)
                .putBoolean(KEY_FSR3_ASYNC, fsr3Async)
                .putBoolean(KEY_FSR3_DIRECT_OUTPUT, fsr3DirectOutput)
                .putBoolean(KEY_FSR3_FORCE, fsr3ForceEnabled)
                .putString(KEY_PRESET, qualityPreset)
                .apply();

        Intent intent = new Intent(this, MainActivity.class);
        intent.putExtra("show_fps", showFps);
        intent.putExtra("stats_log", statsLog);
        intent.putExtra("debug_logs", debugLogs);
        intent.putExtra("dump_frames", dumpFrames);
        intent.putExtra("pc_compat", pcCompat);
        intent.putExtra("fast_present", fastPresent);
        intent.putExtra("skip_readback", skipReadback);
        intent.putExtra("gpu_present_copy", gpuPresentCopy);
        intent.putExtra("scale_hack", scale);
        intent.putExtra("fps_cap", cap);
        intent.putExtra("viewport_stretch", stretch);
        intent.putExtra("present_fsr", fsr);
        intent.putExtra("aa", aa);
        intent.putExtra("present_sharpen", sharpen);
        intent.putExtra("fsr3_ahb_probe", fsr3AhbProbe);
        intent.putExtra("fsr3_force", fsr3ForceEnabled);
        intent.putExtra("fsr3_fg", fsr3FrameGeneration);
        intent.putExtra("fsr3_real_depth", fsr3RealDepth);
        intent.putExtra("fsr3_gpu_depth", fsr3GpuDepth);
        intent.putExtra("fsr3_async", fsr3Async);
        intent.putExtra("fsr3_direct_output", fsr3DirectOutput);
        intent.putExtra("fsr3_diag_readback", fsr3DiagReadback);
        intent.putExtra("fsr3_trace", fsr3Trace);
        intent.putExtra("fsr3_fg_synthetic", fsr3FgSynthetic);
        intent.putExtra("fsr3_fg_probe", fsr3FgProbe);
        String diagnosticEnv = getIntent().getStringExtra("diagnostic_env");
        if (diagnosticEnv != null && !diagnosticEnv.trim().isEmpty()) {
            intent.putExtra("diagnostic_env", diagnosticEnv);
        }
        if (ipaPath != null && !ipaPath.trim().isEmpty()) {
            intent.putExtra("ipa_path", ipaPath);
        }
        startActivity(intent);
        finish();
    }

    private void updateFsr3SelectionStatus() {
        if (fsr3SelectionStatus == null) return;
        updateFsr3ControlEnablement();
        if (!fsr3Supported && !fsr3ForceEnabled) {
            fsr3SelectionStatus.setTextColor(COLOR_WARN);
            fsr3SelectionStatus.setText(
                    "当前选择：不可用。设备未满足基础 Vulkan/ABI/API 条件，将使用原生路径。");
            if (fsr3DepthToggle != null) fsr3DepthToggle.setEnabled(false);
            return;
        }

        boolean depthEnabled = fsr3AhbToggle.isChecked() && fsr3DepthToggle.isChecked();
        fsr3DepthToggle.setEnabled(fsr3AhbToggle.isChecked());
        fsr3GpuDepthToggle.setEnabled(depthEnabled);
        fsr3AsyncToggle.setEnabled(fsr3AhbToggle.isChecked());
        fsr3DirectOutputToggle.setEnabled(
                fsr3AhbToggle.isChecked() && fsr3AsyncToggle.isChecked());
        if (!fsr3AhbToggle.isChecked()) {
            fsr3SelectionStatus.setTextColor(COLOR_MUTED);
            fsr3SelectionStatus.setText(
                    "当前选择：原生路径。FSR3 超分、真实深度和帧生成均不会运行。");
        } else if (!depthEnabled) {
            fsr3SelectionStatus.setTextColor(COLOR_WARN);
            fsr3SelectionStatus.setText(
                    "当前选择：FSR3.1 呈现/超分探针，depth=false。"
                            + "请同时开启“真实深度输入 D16”才能形成当前可验证的最小输入组合。");
        } else {
            fsr3SelectionStatus.setTextColor(COLOR_ACCENT);
            fsr3SelectionStatus.setText(
                    "当前选择：FSR3.1 + 真实 D16 深度（"
                            + (fsr3GpuDepthToggle.isChecked() ? "GPU 直通" : "CPU 读回")
                            + "，" + (fsr3AsyncToggle.isChecked() ? "异步一帧流水线" : "同步等待")
                            + (fsr3DirectOutputToggle.isChecked() ? " + 直接输出" : "")
                            + "）。"
                            + "仍缺 motion/jitter/history/HUD 分层；帧生成保持关闭。");
        }
    }

    private boolean fsr3Usable() {
        return fsr3Supported || fsr3ForceEnabled;
    }

    private void updateFsr3ControlEnablement() {
        boolean usable = fsr3Usable();
        boolean ahb = usable && fsr3AhbToggle != null && fsr3AhbToggle.isChecked();
        boolean depth = ahb && fsr3DepthToggle != null && fsr3DepthToggle.isChecked();
        if (fsr3AhbToggle != null) fsr3AhbToggle.setEnabled(usable);
        if (fsr3DepthToggle != null) fsr3DepthToggle.setEnabled(ahb);
        if (fsr3GpuDepthToggle != null) fsr3GpuDepthToggle.setEnabled(depth);
        if (fsr3AsyncToggle != null) fsr3AsyncToggle.setEnabled(ahb);
        if (fsr3DirectOutputToggle != null) {
            fsr3DirectOutputToggle.setEnabled(
                    ahb && fsr3AsyncToggle != null && fsr3AsyncToggle.isChecked());
        }
    }

    private void addInfoCard(LinearLayout root, String title, String body, int accent) {
        LinearLayout card = new LinearLayout(this);
        card.setOrientation(LinearLayout.VERTICAL);
        card.setPadding(dp(16), dp(12), dp(16), dp(12));
        card.setBackground(panelBackground(COLOR_PANEL, COLOR_LINE));

        TextView heading = text(title, 15, accent, true);
        card.addView(heading);

        TextView detail = text(body, 13, COLOR_MUTED, false);
        detail.setLineSpacing(0f, 1.08f);
        detail.setPadding(0, dp(5), 0, 0);
        card.addView(detail);

        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT);
        params.bottomMargin = dp(8);
        root.addView(card, params);
    }

    private void addSection(LinearLayout root, String label) {
        TextView section = text(label.toUpperCase(), 12, COLOR_ACCENT, true);
        section.setLetterSpacing(0.16f);
        section.setPadding(0, dp(19), 0, dp(7));
        root.addView(section);
    }

    private CheckBox addToggle(LinearLayout root, String title, String detail, boolean checked) {
        CheckBox box = new CheckBox(this);
        box.setText(title + "\n" + detail);
        box.setTextSize(15);
        box.setTextColor(COLOR_TEXT);
        box.setButtonTintList(android.content.res.ColorStateList.valueOf(COLOR_ACCENT));
        box.setLineSpacing(0f, 1.08f);
        box.setGravity(Gravity.TOP | Gravity.START);
        box.setPadding(dp(16), dp(12), dp(16), dp(12));
        box.setBackground(panelBackground(COLOR_PANEL, COLOR_LINE));
        box.setChecked(checked);
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT);
        params.bottomMargin = dp(8);
        root.addView(box, params);
        return box;
    }

    private Button addCycleButton(LinearLayout root, String unused, View.OnClickListener listener) {
        Button button = button("", false);
        button.setGravity(Gravity.START | Gravity.CENTER_VERTICAL);
        button.setOnClickListener(listener);
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, dp(54));
        params.bottomMargin = dp(8);
        root.addView(button, params);
        return button;
    }

    private Button button(String label, boolean primary) {
        Button button = new Button(this);
        button.setText(label);
        button.setTextSize(15);
        button.setAllCaps(false);
        button.setTextColor(primary ? COLOR_BG : COLOR_TEXT);
        button.setTypeface(Typeface.create("sans-serif-medium", Typeface.NORMAL));
        button.setBackground(panelBackground(primary ? COLOR_ACCENT : COLOR_PANEL_ALT,
                primary ? Color.rgb(169, 245, 247) : COLOR_LINE));
        button.setPadding(dp(15), 0, dp(15), 0);
        return button;
    }

    private TextView text(String value, int sizeSp, int color, boolean bold) {
        TextView view = new TextView(this);
        view.setText(value);
        view.setTextSize(sizeSp);
        view.setTextColor(color);
        if (bold) {
            view.setTypeface(Typeface.create("sans-serif", Typeface.BOLD));
        }
        return view;
    }

    private GradientDrawable panelBackground(int fill, int stroke) {
        GradientDrawable drawable = new GradientDrawable();
        drawable.setColor(fill);
        drawable.setCornerRadius(dp(4));
        drawable.setStroke(dp(1), stroke);
        return drawable;
    }

    private int dp(int value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
    }

    @SuppressWarnings("deprecation")
    private void applyImmersiveMode() {
        Window window = getWindow();
        window.setStatusBarColor(Color.TRANSPARENT);
        window.setNavigationBarColor(Color.TRANSPARENT);
        window.addFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN);
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
        window.getDecorView().setSystemUiVisibility(
                View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                        | View.SYSTEM_UI_FLAG_FULLSCREEN
                        | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                        | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                        | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                        | View.SYSTEM_UI_FLAG_LAYOUT_STABLE);
    }
}
