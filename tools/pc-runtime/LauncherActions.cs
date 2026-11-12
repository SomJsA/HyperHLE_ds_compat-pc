using System;
using System.Drawing;
using System.Windows.Forms;

namespace DeadSpaceBootstrap
{
    internal sealed partial class LauncherForm
    {
        private readonly Button exitLauncher = new Button();
        private readonly Button undoChanges = new Button();
        private readonly Button applyChanges = new Button();

        internal sealed class NoWheelComboBox : ComboBox
        {
            protected override void OnMouseWheel(MouseEventArgs e)
            {
                // Never let a wheel tick mutate a setting. Use a click plus
                // arrows/keyboard to choose a value deliberately.
                return;
            }
        }

        private void BuildLauncherExitButton()
        {
            exitLauncher.Text = "退出";
            exitLauncher.Size = new Size(106, 42);
            exitLauncher.Click += delegate { Close(); };
            Controls.Add(exitLauncher);
            undoChanges.Text = "撤销";
            undoChanges.Size = new Size(88, 42);
            undoChanges.Click += delegate {
                try {
                    if (IsSaveEditorPage) { UndoSaveChanges(); return; }
                    LoadSettings(); LoadKeyBindings(); LoadGamepadBindings(); ApplyUiLanguage();
                    status.Text = "已撤销未应用的设置。";
                } catch (Exception error) { status.Text = "撤销失败：" + error.Message; }
            };
            Controls.Add(undoChanges);
            applyChanges.Text = "应用";
            applyChanges.Size = new Size(88, 42);
            applyChanges.Click += delegate {
                try {
                    if (IsSaveEditorPage) { ApplyAllSaveChanges(); return; }
                    SaveSettings(); SaveKeyBindings(); SaveGamepadBindings();
                    status.Text = "设置已应用并保存；下次启动游戏时生效。";
                } catch (Exception error) { status.Text = "应用失败：" + error.Message; }
            };
            Controls.Add(applyChanges);
            Resize += delegate { PositionLauncherActions(); };
            PositionLauncherActions();
        }

        private void PositionLauncherActions()
        {
            LayoutLauncher();
        }
    }
}
