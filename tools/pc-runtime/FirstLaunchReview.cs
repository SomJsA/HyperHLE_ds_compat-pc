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
                summary=FirstLaunchReviewForm.Summary(overlayEnabled.Checked,advancedLighting.Checked,
                    PcRenderResolution.Size(scale),SelectedOutputSize());
                if (resourceProfile != null) summary = resourceProfile.Describe(DeadSpaceUi.UiLanguage.English) + "\r\n\r\n" + summary;
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
        internal static string Summary(bool text,bool lighting,string render,string output)
        {
            if(String.IsNullOrEmpty(output))output=T("跟随内部","Follow internal");
            else output=output.Replace("x"," × ");
            string on=T("开启","On"),off=T("关闭","Off");
            return T("请复核本次启动的画面设置。需要修改时可返回启动器；确认后保存，今后仍可随时调整。",
                "Review your display settings. Return to the launcher to change them, or confirm to save them. You can adjust them again later.")
                +"\r\n\r\n"+T("高清中文文字组件：","HD Chinese text components: ")+(text?on:off)
                +"\r\n"+T("高级光影：","Advanced lighting: ")+(lighting?on:off)
                +"\r\n"+T("内部分辨率：","Internal resolution: ")+render
                +"\r\n"+T("输出分辨率：","Output resolution: ")+output
                +"\r\n\r\n"+T("支持高级光影的 IPA 在 PC 默认开启此项，用于原生高档反光和灯光；实际可用项以本次 IPA 适配说明为准。",
                    "Advanced lighting is on by default on PC for supported IPAs, enabling native high-tier reflections and lights. Available features are listed in this IPA's compatibility summary.")
                +"\r\n\r\n"+T("旧版IPA按其自带资源与渲染能力运行，可不安装外置高清纹理。1.0.3/1.0.1沿用原生触摸界面，1.3.34专用的增强操控和高清文字不适用于旧版。高级光影开关不会为旧包补入缺少的高档渲染资源。",
                    "Legacy IPAs use their own resources and renderer; external HD textures are optional. Editions 1.0.3/1.0.1 retain the original touch UI. The enhanced controls and HD text for 1.3.34 do not apply to these editions. Advanced lighting cannot add high-tier content missing from an older IPA.")
                +"\r\n\r\n"+T("高清文字目前为中文。可关闭高清中文组件，使用游戏原生文字与语言设置。欢迎多语言用户投稿；若暂无投稿，我们计划在R8之前支持其他语言的高清文字组件。",
                    "HD text is currently Chinese. Disable the HD Chinese components to use the game's native text and language settings. Contributions for other languages are welcome. If none are contributed, we plan to support HD text in other languages before R8.");
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
