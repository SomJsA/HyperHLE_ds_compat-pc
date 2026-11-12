using System;
using System.Drawing;
using System.Windows.Forms;

namespace DeadSpaceBootstrap
{
    internal sealed partial class LauncherForm
    {
        private float uiScale = 1f;
        private bool layoutReady, layingOut;
        private readonly FlowLayoutPanel footerActions = new FlowLayoutPanel();
        private bool IsSaveEditorPage { get { return saveEditorPage != null && selectedPageIndex < pages.Count && pages[selectedPageIndex] == saveEditorPage; } }
        private int Px(int value) { return (int)Math.Round(value * uiScale); }
        private void InitializeResponsiveLayout()
        {
            using (var graphics = CreateGraphics()) uiScale = Math.Max(1f, graphics.DpiX / 96f);
            // Bounds are authored in 96-DPI units. Do this once, after every
            // control exists, instead of consuming autoscale before BuildShell.
            if (Math.Abs(uiScale - 1f) > .001f) Scale(new SizeF(uiScale, uiScale));
            footerActions.WrapContents = true; footerActions.AutoSize = false;
            foreach (Control control in new Control[] {restoreDefaults, undoChanges, applyChanges, exitLauncher, stopGame, launch}) {
                control.Anchor = AnchorStyles.None;
                control.Margin = new Padding(0,0,Px(8),Px(6));
                footerActions.Controls.Add(control);
            }
            Controls.Add(footerActions); footerActions.BringToFront();
            stopGame.VisibleChanged += delegate { LayoutLauncher(); };
            Resize += delegate { LayoutLauncher(); };
            layoutReady = true; UpdateFooterContext(); LayoutLauncher();
        }
        private void UpdateFooterContext()
        {
            if (!layoutReady) return;
            bool about = selectedPageIndex == 0;
            restoreDefaults.Visible = !about && !IsSaveEditorPage;
            undoChanges.Visible = !about; applyChanges.Visible = !about;
            // The same bottom Apply owns all save edits on page06. No duplicate
            // write button is left inside the scrolling save editor.
            LayoutLauncher();
        }
        private void UndoSaveChanges()
        {
            bool hadEditor = editorDocument != null, hadPlugins = loadedSave != null;
            saveGrid.CancelEdit(); pendingSaveChanges.Clear();
            if (hadEditor) ReadEditorSave();
            if (hadPlugins) ReadCurrentSave();
            status.Text = DeadSpaceUi.UiLanguage.English ? "Unapplied save edits discarded; nothing was written." : "已撤销未应用的存档修改，未写入文件。";
        }
        private Control Sidebar(string tag)
        {
            foreach (Control c in Controls) if (Convert.ToString(c.Tag)==tag) return c;
            return null;
        }
        private void LayoutLauncher()
        {
            if (!layoutReady || layingOut) return;
            layingOut = true;
            try {
                int x=Px(224), width=Math.Max(Px(320),ClientSize.Width-x-Px(24));
                languageToggle.SetBounds(ClientSize.Width-Px(122),Px(18),Px(94),Px(32));
                int top=Px(18), sideWidth=Px(184);
                foreach (string tag in new[] {"sidebar-title","sidebar-subtitle","sidebar-egg"}) {
                    Control c=Sidebar(tag); if(c==null) continue;
                    int height=Math.Max(Px(tag=="sidebar-title"?54:24),c.GetPreferredSize(new Size(sideWidth,0)).Height);
                    c.SetBounds(Px(28),top,sideWidth,height); top=c.Bottom+Px(6);
                }
                navigationHost.SetBounds(Px(24),top+Px(10),sideWidth,Math.Min(Px(350),Math.Max(Px(110),ClientSize.Height-top-Px(46))));
                for (int i=0;i<navigation.Count;i++) navigation[i].SetBounds(0,i*Px(43),sideWidth,Px(38));
                int statusHeight=Px(28); status.SetBounds(x,ClientSize.Height-statusHeight-Px(4),width,statusHeight);
                int footerHeight=footerActions.GetPreferredSize(new Size(width,0)).Height;
                footerHeight=Math.Max(Px(54),footerHeight);
                footerActions.SetBounds(x,status.Top-footerHeight-Px(4),width,footerHeight);
                mainHost.SetBounds(x,Px(64),width,Math.Max(Px(150),footerActions.Top-Px(78)));
                Control poster=Sidebar("sidebar-poster"), build=Sidebar("sidebar-build");
                bool roomy=ClientSize.Height-navigationHost.Bottom>Px(200);
                if(poster!=null) { poster.Visible=roomy; if(roomy)poster.SetBounds(Px(48),navigationHost.Bottom+Px(18),Px(128),Px(128)); }
                if(build!=null) { build.Visible=roomy; if(roomy)build.SetBounds(Px(28),navigationHost.Bottom+Px(152),sideWidth,Px(68)); }
            } finally { layingOut=false; }
        }
    }
}
