using System;
using System.Drawing;
using System.Windows.Forms;

namespace DeadSpaceBootstrap
{
    internal sealed partial class LauncherForm
    {
        private readonly CheckBox advancedLighting = new CheckBox();
        private bool launchReviewCompleted;
        private bool ReviewFirstLaunch()
        {
            if (launchReviewCompleted) return true;
            string summary;
            try {
                int scale=SelectedRenderScale();
                DeadSpaceResources.IpaLanguage selectedLanguage = SelectedTextLanguage();
                string language = selectedLanguage == null ? (DeadSpaceUi.UiLanguage.English ? "Native game language" : "原生语言设置") : selectedLanguage.Display(DeadSpaceUi.UiLanguage.English);
                summary=FirstLaunchReviewForm.Summary(overlayEnabled.Checked, language, advancedLighting.Checked,
                    PcRenderResolution.Size(scale),SelectedOutputSize());
                if (selectedLanguage != null && !selectedLanguage.Complete)
                    summary += "\r\n" + (DeadSpaceUi.UiLanguage.English ? "This IPA language bank is partial; existing Japanese entries stay Japanese and missing entries use English to avoid placeholder text." : "该 IPA 的语言资源不完整；已有日文条目继续使用日文，缺失条目回退英语，避免出现占位符。");
                if (resourceProfile != null) summary += "\r\n\r\n" + resourceProfile.Describe(DeadSpaceUi.UiLanguage.English);
            } catch (InvalidOperationException error) { status.Text=error.Message; return false; }
            using (var review=new FirstLaunchReviewForm(summary)) {
                if (review.ShowDialog(this)!=DialogResult.OK) {SelectPage(1);return false;}
            }
            launchReviewCompleted=true;
            return true;
        }
    }

    internal sealed class FirstLaunchReviewForm : Form
    {
        internal static string T(string zh,string en) { return DeadSpaceUi.UiLanguage.English ? en : zh; }
        internal static string Summary(bool text,string language,bool lighting,string render,string output)
        {
            if(String.IsNullOrEmpty(output))output=T("跟随内部","Follow internal");
            else output=output.Replace("x"," × ");
            string on=T("开启","On"),off=T("关闭","Off");
            return T("请复核本次启动的画面设置。需要修改时可返回启动器；确认后保存，今后仍可随时调整。",
                "Review your display settings. Return to the launcher to change them, or confirm to save them. You can adjust them again later.")
                +"\r\n"+T("高清文字组件：","HD text component: ")+(text?on:off)
                +"\r\n"+T("高清语言：","HD language: ")+(text?language:T("原生语言设置","Native game language"))
                +"\r\n"+T("高级光影：","Advanced lighting: ")+(lighting?on:off)
                +"\r\n"+T("内部分辨率：","Internal resolution: ")+render
                +"\r\n"+T("输出分辨率：","Output resolution: ")+output
                +"\r\n\r\n"+T("支持高级光影的 IPA 在 PC 默认开启此项，用于原生高档反光和灯光；实际可用项以本次 IPA 适配说明为准。",
                    "Advanced lighting is on by default on PC for supported IPAs, enabling native high-tier reflections and lights. Available features are listed in this IPA's compatibility summary.")
                +"\r\n\r\n"+T("旧版IPA按其自带资源与渲染能力运行，可不安装外置高清纹理。1.0.3/1.0.1沿用原生触摸界面，1.3.34专用的增强操控和高清文字不适用于旧版。高级光影开关不会为旧包补入缺少的高档渲染资源。",
                    "Legacy IPAs use their own resources and renderer; external HD textures are optional. Editions 1.0.3/1.0.1 retain the original touch UI. The enhanced controls and HD text for 1.3.34 do not apply to these editions. Advanced lighting cannot add high-tier content missing from an older IPA.")
                +"\r\n\r\n"+T("中文选项使用贴吧汉化组高清组件；其他选项使用 IPA 内置原生文字，并在启动时传入对应语言环境。关闭组件则完全回归游戏原生语言设置。",
                    "Chinese uses the Tieba Translation Group HD component. Other choices use the IPA's embedded native text and pass the matching language environment at launch. Disable the component to use the game's native language settings.")
                +"\r\n"+T("已有存档的 settings.sb 会在启动前按所选语言切换，并自动保留备份。",
                    "If a save already exists, settings.sb is switched before launch and a backup is kept automatically.");
        }
        internal FirstLaunchReviewForm(string summary)
        {
            Name="FirstLaunchReviewForm";
            Text=T("首次启动 · 画面设置复核","First launch · Review display settings");
            StartPosition=FormStartPosition.CenterParent;
            AutoScaleMode=AutoScaleMode.Dpi;ClientSize=new Size(660,450);MinimumSize=new Size(560,380);
            BackColor=Color.FromArgb(19,30,40);ForeColor=Color.FromArgb(221,232,237);
            Font=new Font("Segoe UI",11);ShowInTaskbar=false;MinimizeBox=false;MaximizeBox=false;
            var body=new TextBox { Name="ReviewSummary",ReadOnly=true,Multiline=true,ScrollBars=ScrollBars.Vertical,
                BorderStyle=BorderStyle.None,BackColor=BackColor,ForeColor=ForeColor,Text=summary,Dock=DockStyle.Fill };
            var buttons=new FlowLayoutPanel { Dock=DockStyle.Bottom,Height=66,FlowDirection=FlowDirection.RightToLeft,Padding=new Padding(12) };
            var confirm=new Button { Name="ConfirmLaunchSettings",Text=T("确认并启动","Confirm and launch"),DialogResult=DialogResult.OK,AutoSize=true,MinimumSize=new Size(160,38) };
            var settings=new Button { Name="ReturnToSettings",Text=T("返回设置","Back to settings"),DialogResult=DialogResult.Cancel,AutoSize=true,MinimumSize=new Size(150,38) };
            foreach(Button button in new[]{confirm,settings}) {button.BackColor=Color.FromArgb(10,17,24);button.ForeColor=ForeColor;button.FlatStyle=FlatStyle.Flat;buttons.Controls.Add(button);}
            var textHost=new Panel {Dock=DockStyle.Fill,Padding=new Padding(22)};textHost.Controls.Add(body);
            Controls.Add(textHost);Controls.Add(buttons);AcceptButton=confirm;CancelButton=settings;
        }
    }
}
