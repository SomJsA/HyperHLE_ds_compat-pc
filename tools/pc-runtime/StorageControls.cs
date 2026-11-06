using System;
using System.Drawing;
using System.IO;
using System.Windows.Forms;

namespace DeadSpaceBootstrap
{
    internal sealed partial class LauncherForm
    {
        private readonly ComboBox saveMode = new ComboBox();
        private readonly Label storagePath = new Label();

        private string SelectedDataRoot()
        {
            string root=SaveStorage.Root(DistributionLayout.PortableData(baseDir),
                documentsDirectory, saveMode.SelectedIndex == 1);
            if(saveMode.SelectedIndex==1)DocumentsSettings.RequireCompatibleLayout(root);
            return root;
        }

        private void BuildStorageControls(Control page)
        {
            AddComboRow(page, "存档位置", 376, saveMode, new[] { "便携（启动器目录）", "文档（当前用户）" });
            storagePath.Location = new Point(30, 418); storagePath.Size = new Size(675, 70);
            page.Controls.Add(storagePath);
            var copy = new Button { Text = "从另一模式复制存档到所选模式", Location = new Point(30, 500), Size = new Size(300, 38) };
            page.Controls.Add(copy);
            LabelText(page, "切换位置不会自动合并进度；空目录会新建进度。复制只接受空目标，保留源档并逐文件校验。\r\n复制方向：另一模式 → 当前所选模式。请先退出游戏；已有目标存档时拒绝覆盖。", 30, 552, 675, 65);
            LabelText(page, "三个槽位，一个 ZIP：同时包含全部现有档案和公共设置，可与 Android 互导。\r\n导入前自动备份当前三槽位；请先退出游戏。",30,630,675,55);
            var exportZip=new Button {Text="导出三槽位 ZIP",Location=new Point(30,696),Size=new Size(300,38)};
            var importZip=new Button {Text="导入 ZIP 并恢复三槽位",Location=new Point(348,696),Size=new Size(330,38)};
            page.Controls.Add(exportZip);page.Controls.Add(importZip);
            exportZip.Click+=delegate {using(var dialog=new SaveFileDialog {Filter="三槽位存档包 (*.zip)|*.zip",FileName="DeadSpace-Three-Slots-"+DateTime.Now.ToString("yyyyMMdd-HHmmss")+".zip",OverwritePrompt=false})
                {if(dialog.ShowDialog(this)!=DialogResult.OK)return;try {SaveBundle.Export(SelectedDataRoot(),dialog.FileName);status.Text="已导出全部三槽位及公共设置："+dialog.FileName;}catch(Exception error){status.Text="未导出："+error.Message;}}};
            importZip.Click+=delegate {using(var dialog=new OpenFileDialog {Filter="三槽位存档包 (*.zip)|*.zip",Title="恢复整个 ZIP 的三个槽位；当前状态会先备份"})
                {if(dialog.ShowDialog(this)!=DialogResult.OK)return;try {string backup=SaveBundle.Import(SelectedDataRoot(),dialog.FileName);loadedSave=null;writeSave.Enabled=false;InvalidateEditor();status.Text="已恢复三槽位。"+(backup==null?"":" 导入前备份："+backup);}catch(Exception error){status.Text="未导入："+error.Message;}}};
            saveMode.SelectedIndexChanged += delegate
            {
                try { storagePath.Text = "启动器配置："+ConfigurationDirectory+"\r\n原生配置与存档：" + SaveStorage.ProfilePath(SelectedDataRoot()); }
                catch (Exception error) { storagePath.Text = error.Message; }
                loadedSave = null; writeSave.Enabled = false;
                InvalidateEditor();
                saveStatus.Text = "存档位置已切换，请重新读取插件设置。";
            };
            copy.Click += delegate
            {
                try
                {
                    string documents = documentsDirectory;
                    string source = SaveStorage.Root(DistributionLayout.PortableData(baseDir), documents, saveMode.SelectedIndex != 1);
                    int count = SaveStorage.CopyProfile(source, SelectedDataRoot());
                    status.Text = "已复制并校验 " + count + " 个文件，源存档保留。";
                }
                catch (Exception error) { status.Text = "未迁移：" + error.Message; }
            };
        }
    }
}
