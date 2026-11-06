using System;
using System.Diagnostics;
using System.Drawing;
using System.Runtime.InteropServices;
using System.Windows.Forms;

namespace DeadSpaceBootstrap
{
    internal sealed class QuitConfirmation : Form
    {
        private readonly Button cancel = new Button();
        private readonly Button quit = new Button();
        private readonly Color cyan = Color.FromArgb(119, 230, 243);
        private readonly Timer padTimer = new Timer();
        private ushort previousButtons;

        [StructLayout(LayoutKind.Sequential)] private struct Pad
        { public ushort Buttons; public byte LeftTrigger, RightTrigger; public short LX, LY, RX, RY; }
        [StructLayout(LayoutKind.Sequential)] private struct PadState
        { public uint Packet; public Pad Gamepad; }
        [DllImport("xinput1_4.dll")] private static extern uint XInputGetState(uint index, out PadState state);
        private sealed class DialogOwner : IWin32Window
        { public IntPtr Handle { get; private set; } public DialogOwner(IntPtr handle) { Handle = handle; } }

        internal static int Run(int processId)
        {
            IntPtr handle = IntPtr.Zero;
            try { handle = Process.GetProcessById(processId).MainWindowHandle; } catch { }
            using (var dialog = new QuitConfirmation())
                return dialog.ShowDialog(new DialogOwner(handle)) == DialogResult.Yes ? 10 : 0;
        }

        private QuitConfirmation()
        {
            Text = "Dead Space · 退出游戏";
            FormBorderStyle = FormBorderStyle.None;
            StartPosition = FormStartPosition.CenterParent;
            ClientSize = new Size(560, 276);
            BackColor = Color.FromArgb(5, 17, 23);
            ForeColor = Color.FromArgb(215, 248, 251);
            Font = new Font("Microsoft YaHei UI", 11);
            ShowInTaskbar = false;
            TopMost = true;
            KeyPreview = true;
            var eyebrow = new Label { Text = "DEAD SPACE  //  SYSTEM", ForeColor = cyan,
                Font = new Font("Consolas", 10), Location = new Point(30, 27), AutoSize = true };
            var heading = new Label { Text = "退出游戏？", Font = new Font(Font.FontFamily, 23),
                Location = new Point(28, 64), AutoSize = true };
            var body = new Label { Text = "进度以最近一次游戏内自动保存为准。", ForeColor = Color.FromArgb(158, 187, 194),
                Location = new Point(31, 126), AutoSize = true };
            Controls.AddRange(new Control[] { eyebrow, heading, body });
            SetupButton(cancel, "取消  [Q / B]", new Point(30, 194), DialogResult.Cancel);
            SetupButton(quit, "退出游戏  [E / A]", new Point(292, 194), DialogResult.Yes);
            CancelButton = cancel;
            Shown += (s, e) => { Activate(); BringToFront(); cancel.Select(); };
            KeyDown += (s, e) => {
                if (e.KeyCode == Keys.Q || e.KeyCode == Keys.Escape) Finish(DialogResult.Cancel);
                else if (e.KeyCode == Keys.E || e.KeyCode == Keys.Enter) Finish(quit.Focused ? DialogResult.Yes : DialogResult.Cancel);
                else if (e.KeyCode == Keys.Left || e.KeyCode == Keys.Right) { if (cancel.Focused) quit.Select(); else cancel.Select(); }
                else return;
                e.Handled = true; e.SuppressKeyPress = true;
            };
            // Poll the existing pad without changing the game's saved bindings.
            padTimer.Interval = 35;
            padTimer.Tick += (s, e) => {
                try {
                    PadState state; ushort buttons = 0;
                    for (uint i = 0; i < 4; i++) if (XInputGetState(i, out state) == 0) { buttons = state.Gamepad.Buttons; break; }
                    ushort down = (ushort)(buttons & ~previousButtons); previousButtons = buttons;
                    if ((down & 0x2000) != 0) Finish(DialogResult.Cancel);
                    else if ((down & 0x1000) != 0) { if (quit.Focused) Finish(DialogResult.Yes); else Finish(DialogResult.Cancel); }
                    else if ((down & 0x000c) != 0) { if (cancel.Focused) quit.Select(); else cancel.Select(); }
                } catch (DllNotFoundException) { padTimer.Stop(); }
            };
            padTimer.Start();
        }

        private void SetupButton(Button button, string text, Point at, DialogResult result)
        {
            button.Text = text; button.Location = at; button.Size = new Size(238, 48);
            button.FlatStyle = FlatStyle.Flat; button.FlatAppearance.BorderColor = Color.FromArgb(50, 104, 117);
            button.FlatAppearance.MouseOverBackColor = Color.FromArgb(21, 54, 65);
            button.BackColor = Color.FromArgb(10, 31, 40); button.ForeColor = ForeColor;
            button.GotFocus += (s, e) => { button.FlatAppearance.BorderColor = cyan; button.BackColor = Color.FromArgb(21, 54, 65); };
            button.LostFocus += (s, e) => { button.FlatAppearance.BorderColor = Color.FromArgb(50, 104, 117); button.BackColor = Color.FromArgb(10, 31, 40); };
            button.Click += (s, e) => Finish(result);
            Controls.Add(button);
        }
        private void Finish(DialogResult result) { DialogResult = result; Close(); }
        protected override void OnPaint(PaintEventArgs e)
        {
            base.OnPaint(e);
            using (var pen = new Pen(cyan)) e.Graphics.DrawRectangle(pen, 0, 0, ClientSize.Width - 1, ClientSize.Height - 1);
        }
        protected override void Dispose(bool disposing)
        { if (disposing) padTimer.Dispose(); base.Dispose(disposing); }
    }
}
