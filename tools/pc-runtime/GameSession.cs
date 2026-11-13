using System;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Windows.Forms;

namespace DeadSpaceBootstrap
{
    internal sealed partial class LauncherForm
    {
        private readonly Button launch = new Button();
        private readonly Button stopGame = new Button();
        private readonly Timer gameTimer = new Timer { Interval = 500 };
        private Process gameProcess;
        private DateTime closeRequested;
        private delegate bool WindowVisitor(IntPtr window, IntPtr value);
        [DllImport("user32.dll")] private static extern bool EnumWindows(WindowVisitor visitor, IntPtr value);
        [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
        [DllImport("user32.dll")] private static extern IntPtr GetWindow(IntPtr window, uint command);
        [DllImport("user32.dll")] private static extern int GetWindowTextLength(IntPtr window);
        [DllImport("user32.dll", SetLastError=true)] private static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);

        private static bool RequestGameClose(Process process)
        {
            if (process.CloseMainWindow()) return true;
            // Process.MainWindowHandle excludes hidden windows. An SDL window
            // may be hidden during a fullscreen transition; still request its
            // own normal close path, without terminating the process.
            bool sent = false; uint wanted = (uint)process.Id;
            EnumWindows(delegate(IntPtr window, IntPtr unused)
            {
                uint pid; GetWindowThreadProcessId(window, out pid);
                if (pid == wanted && GetWindow(window, 4) == IntPtr.Zero && GetWindowTextLength(window) > 0)
                { sent = PostMessage(window, 0x0010, IntPtr.Zero, IntPtr.Zero); if (sent) return false; }
                return true;
            }, IntPtr.Zero);
            return sent;
        }

        private void WatchGame()
        {
            gameTimer.Tick += delegate { RefreshGameState(); };
            FormClosed += delegate { gameTimer.Stop(); gameTimer.Dispose(); if (gameProcess != null) gameProcess.Dispose(); };
            gameTimer.Start();
            RefreshGameState();
        }

        private bool RefreshGameState()
        {
            if (gameProcess != null)
            {
                try { gameProcess.Refresh(); if (gameProcess.HasExited) { RecordDiagnosticExit(gameProcess); gameProcess.Dispose(); gameProcess = null; closeRequested = default(DateTime); status.Text = "游戏已退出，可以重新启动。"; } }
                catch (InvalidOperationException) { gameProcess.Dispose(); gameProcess = null; }
            }
            bool anyRunning = gameProcess != null;
            foreach (Process candidate in Process.GetProcessesByName("touchHLE"))
            {
                bool retained = false;
                try
                {
                    if (!candidate.HasExited)
                    {
                        anyRunning = true;
                        if (gameProcess == null && string.Equals(Path.GetFullPath(candidate.MainModule.FileName), Path.GetFullPath(Path.Combine(DistributionLayout.Game(baseDir), "touchHLE.exe")), StringComparison.OrdinalIgnoreCase))
                        { gameProcess = candidate; retained = true; }
                    }
                }
                catch (System.ComponentModel.Win32Exception) { }
                catch (InvalidOperationException) { }
                finally { if (!retained) candidate.Dispose(); }
            }
            launch.Text = DeadSpaceUi.UiLanguage.Translate(anyRunning ? "已启动" : "启动游戏  →");
            launch.Enabled = !anyRunning && !diagnosticExporting;
            UpdateDiagnosticStatus();
            stopGame.Enabled = gameProcess != null;
            return anyRunning;
        }

        private void StopGame()
        {
            RefreshGameState();
            if (gameProcess == null) return;
            try
            {
                gameProcess.Refresh();
                if (closeRequested != default(DateTime) && (DateTime.UtcNow - closeRequested).TotalSeconds >= 3 && !gameProcess.Responding)
                {
                    if (MessageBox.Show(this, "游戏仍在运行。是否强制停止？未保存的进度可能丢失。", "停止游戏", MessageBoxButtons.YesNo, MessageBoxIcon.Warning) == DialogResult.Yes)
                    { if (!gameProcess.HasExited) gameProcess.Kill(); }
                    return;
                }
                if (RequestGameClose(gameProcess))
                { closeRequested = DateTime.UtcNow; status.Text = "已请求退出；请在游戏中确认。取消退出后仍可继续游戏。"; }
                else
                { closeRequested = DateTime.UtcNow; status.Text = "尚未找到游戏窗口，请稍后重试停止。"; }
            }
            catch (InvalidOperationException) { RefreshGameState(); }
            catch (System.ComponentModel.Win32Exception error) { status.Text = "停止失败：" + error.Message; }
        }
    }
}
