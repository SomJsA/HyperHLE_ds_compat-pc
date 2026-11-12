using System;
using System.Drawing;
using System.Diagnostics;
using System.Windows.Forms;
using DeadSpaceUi;

namespace DeadSpaceBootstrap
{
    // Confirm before creating the game process. Cancelling leaves saves,
    // configuration and the native pause/resume state untouched.
    internal sealed class FlickerWarningForm : Form
    {
        internal FlickerWarningForm()
        {
            Text = UiLanguage.Translate("高闪烁场景提醒");
            Font = new Font("Microsoft YaHei UI", 10.5f);
            AutoScaleMode = AutoScaleMode.Dpi;
            ClientSize = new Size(700, 332);
            StartPosition = FormStartPosition.CenterParent;
            FormBorderStyle = FormBorderStyle.FixedDialog;
            MaximizeBox = false;
            MinimizeBox = false;
            ShowInTaskbar = false;
            BackColor = Color.FromArgb(9, 18, 25);
            ForeColor = Color.Gainsboro;

            var body = new Label {
                Text = UiLanguage.Translate("本游戏含高强度闪烁场景，这是原游戏为营造氛围设计的原生行为。\r\n\r\n游玩时如有不适，请立即停止游玩并及时休息。\r\n\r\n实验性护眼模式会改变黑色、暗部和原有氛围，影响观感；默认关闭，可在“画质与显示”中调整。此开关不会消除所有闪烁。"),
                Location = new Point(24, 22), Size = new Size(652, 236)
            };
            var back = new Button {
                Text = UiLanguage.Translate("返回启动器"),
                DialogResult = DialogResult.Cancel,
                Location = new Point(208, 278), Size = new Size(208, 36)
            };
            var start = new Button {
                Text = UiLanguage.Translate("我已了解，开始游戏"),
                DialogResult = DialogResult.OK,
                Location = new Point(432, 278), Size = new Size(244, 36)
            };
            var tieba = new LinkLabel {
                Text = "死亡空间贴吧汉化组 / Tieba translation group",
                Location = new Point(24, 286), Size = new Size(176, 24),
                LinkColor = Color.FromArgb(98, 218, 232), ActiveLinkColor = Color.White,
                AutoSize = false
            };
            tieba.Links.Add(0, tieba.Text.Length, "https://tieba.baidu.com/f?kw=死亡空间");
            tieba.LinkClicked += delegate(object sender, LinkLabelLinkClickedEventArgs e) {
                try { Process.Start(new ProcessStartInfo { FileName = Convert.ToString(e.Link.LinkData), UseShellExecute = true }); } catch (System.Exception) { }
            };
            Controls.Add(body);
            Controls.Add(tieba);
            Controls.Add(back);
            Controls.Add(start);
            AcceptButton = start;
            CancelButton = back;
        }
    }
}
