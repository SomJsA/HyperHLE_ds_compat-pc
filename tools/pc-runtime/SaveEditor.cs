using System;
using System.Collections.Generic;
using System.Drawing;
using System.Globalization;
using System.IO;
using System.Security.Cryptography;
using System.Windows.Forms;

namespace DeadSpaceBootstrap
{
    internal sealed partial class LauncherForm
    {
        private readonly ComboBox editorSlot = new NoWheelComboBox(), editorKind = new NoWheelComboBox(), editorGroup = new NoWheelComboBox();
        private readonly DataGridView saveGrid = new DataGridView();
        private readonly Label editorSummary = new DeadSpaceUi.UiLanguage.LocalizedLabel();
        private readonly Dictionary<string, string> pendingSaveChanges = new Dictionary<string, string>();
        private SaveDocument editorDocument;
        private string editorImportedDirectory;
        private bool populatingSaveGrid;

        private void BuildSaveEditor(Control page)
        {
            Heading(page, "自由字段修改（下）", "读取后按分组编辑全部可编辑字段；金钱 / 信用点、能量节点和大脉冲可在资源与武器分组中修改。修改前请先导出 ZIP。", 22);
            foreach (var box in new[] { editorSlot, editorKind, editorGroup }) box.DropDownStyle = ComboBoxStyle.DropDownList;
            editorSlot.Items.AddRange(new[] { "存档 1（00）", "存档 2（01）", "存档 3（02）" });
            editorKind.Items.AddRange(new[] { "检查点", "章节存档", "生存存档", "共享设置 / 内购插件", "共享章节 / 成就" });
            editorSlot.Location = new Point(30, 96); editorSlot.Size = new Size(130, 28);
            editorKind.Location = new Point(170, 96); editorKind.Size = new Size(170, 28);
            page.Controls.Add(editorSlot); page.Controls.Add(editorKind);
            var read = new Button { Text = "读取", Location = new Point(350, 94), Size = new Size(90, 32) };
            read.Click += delegate { ReadEditorSave(); }; page.Controls.Add(read);
            var import = new Button { Text = "从备份创建副本", Location = new Point(450, 94), Size = new Size(170, 32) };
            import.Click += delegate { ImportEditorCopy(); }; page.Controls.Add(import);
            editorSummary.Location = new Point(30, 134); editorSummary.Size = new Size(665, 46);
            editorSummary.Text = "每次只写当前文件；检查点和章节档分别保存。读取后可编辑资源、武器、已有背包格与真实内购字段。";
            page.Controls.Add(editorSummary);
            saveGrid.Location = new Point(30, 188); saveGrid.Size = new Size(680, 232);
            saveGrid.AllowUserToAddRows = false; saveGrid.AllowUserToDeleteRows = false; saveGrid.RowHeadersVisible = false;
            saveGrid.AutoGenerateColumns = false; saveGrid.BackgroundColor = Shell;
            saveGrid.DefaultCellStyle.BackColor = Surface; saveGrid.DefaultCellStyle.ForeColor = Ink;
            saveGrid.DefaultCellStyle.SelectionBackColor = Color.FromArgb(40, 80, 95);
            saveGrid.DefaultCellStyle.SelectionForeColor = Ink;
            saveGrid.Columns.Add(new DataGridViewTextBoxColumn { Name = "field", HeaderText = "字段", Width = 355, ReadOnly = true });
            saveGrid.Columns.Add(new DataGridViewTextBoxColumn { Name = "current", HeaderText = "当前值", Width = 135, ReadOnly = true });
            saveGrid.Columns.Add(new DataGridViewTextBoxColumn { Name = "edited", HeaderText = "修改为", Width = 155 });
            saveGrid.CellEndEdit += delegate(object sender, DataGridViewCellEventArgs e)
            {
                if (populatingSaveGrid || editorDocument == null || e.RowIndex < 0 || e.ColumnIndex != 2) return;
                var row = saveGrid.Rows[e.RowIndex]; string path = (string)row.Tag;
                string value = Convert.ToString(row.Cells[2].Value, CultureInfo.InvariantCulture).Trim();
                if (value.Length == 0 || value == editorDocument.Values[path].Value) pendingSaveChanges.Remove(path);
                else pendingSaveChanges[path] = value;
            };
            page.Controls.Add(saveGrid);
            editorGroup.Items.AddRange(new[] { "资源与升级", "武器", "背包", "内购插件", "章节与周目", "共享设置", "进度（只读）", "全部" });
            editorGroup.Location = new Point(30, 430); editorGroup.Size = new Size(165, 28);
            editorGroup.SelectedIndexChanged += delegate { if (!DeadSpaceUi.UiLanguage.IsApplying) PopulateSaveGrid(); }; page.Controls.Add(editorGroup);
            LabelText(page, "帮助：0/1 为开关；物品类型须在本档中已有。只写当前文件，保存前自动备份并检查源文件未变化；不确定的数值不要修改。", 210, 433, 495, 80);
            var preview = new Button { Text = "预览更改", Location = new Point(30, 526), Size = new Size(140, 32) };
            preview.Click += delegate { PreviewEditorChanges(); }; page.Controls.Add(preview);
            var active = new Button { Text = "返回所选存档位置", Location = new Point(185, 526), Size = new Size(165, 32) };
            active.Click += delegate { editorImportedDirectory = null; InvalidateEditor(); ReadEditorSave(); }; page.Controls.Add(active);
            var restore = new Button { Text = "恢复整组备份", Location = new Point(365, 526), Size = new Size(155, 32) };
            restore.Click += delegate { RestoreEditorBackup(); }; page.Controls.Add(restore);
            editorSlot.SelectedIndexChanged += delegate { InvalidateEditor(); };
            editorKind.SelectedIndexChanged += delegate { editorSlot.Enabled = editorKind.SelectedIndex < 3; InvalidateEditor(); };
            editorSlot.SelectedIndex = 0; editorKind.SelectedIndex = 0; editorGroup.SelectedIndex = 0;
            foreach (Control control in page.Controls)
            {
                if (control is Label && control.Top == 64) control.Height = 44;
                else if (control.Top >= 486) control.Top += 72;
                else if (control.Top >= 94) control.Top += 24;
            }
        }

        private void InvalidateEditor()
        {
            if (DeadSpaceUi.UiLanguage.IsApplying) return;
            saveGrid.CancelEdit();
            editorDocument = null; pendingSaveChanges.Clear(); saveGrid.Rows.Clear();
            editorSummary.Text = "文件或位置已切换，请点击读取。";
        }
        private string EditorFile()
        {
            string directory = editorImportedDirectory ?? Path.Combine(SaveStorage.ProfilePath(SelectedDataRoot()), "Documents", "var");
            string[] kinds = { "CheckpointSave.sb", "LevelSave.sb", "SurvivalSave.sb", "settings.sb", "AchievementsSave.sb" };
            int kind = Math.Max(0, editorKind.SelectedIndex);
            return Path.Combine(directory, kind < 3 ? editorSlot.SelectedIndex.ToString("00", CultureInfo.InvariantCulture) + "_" + kinds[kind] : kinds[kind]);
        }
        private void ReadEditorSave()
        {
            try
            {
                saveGrid.CancelEdit();
                pendingSaveChanges.Clear(); editorDocument = new SaveDocument(EditorFile());
                editorSummary.Text = (editorImportedDirectory == null ? "所选存档位置：" : "实验副本：") + editorDocument.FilePath + "\r\nSHA256 " + editorDocument.Sha256.Substring(0, 16) + "…；仅编辑此文件，读取后保存会检查源文件未变。";
                if (editorKind.SelectedIndex >= 3) editorGroup.SelectedIndex = editorKind.SelectedIndex == 3 ? 3 : 4;
                else if (editorKind.SelectedIndex == 2) editorGroup.SelectedIndex = 7;
                PopulateSaveGrid();
            }
            catch (Exception error) { editorDocument = null; saveGrid.Rows.Clear(); status.Text = "无法读取存档：" + error.Message; }
        }
        private void PopulateSaveGrid()
        {
            if (editorDocument == null) return;
            saveGrid.EndEdit(); populatingSaveGrid = true;
            try
            {
                saveGrid.Rows.Clear();
                // Category identity is independent of the translated caption.
                string[] groups = { "资源与升级", "武器", "背包", "内购插件", "章节与周目", "共享设置", "进度（只读）", "全部" };
                string group = groups[Math.Max(0, editorGroup.SelectedIndex)];
                var paths = new List<string>(editorDocument.Values.Keys); paths.Sort(StringComparer.Ordinal);
                foreach (string path in paths)
                {
                    var field = editorDocument.Values[path]; if (group != "全部" && field.Group != group) continue;
                    string pending; pendingSaveChanges.TryGetValue(path, out pending);
                    int index = saveGrid.Rows.Add(DeadSpaceUi.UiLanguage.TranslateEditorLabel(EditorLabel(field)), field.Value, pending ?? ""); var row = saveGrid.Rows[index]; row.Tag = path;
                    row.Cells[0].ToolTipText = path;
                    row.Cells[2].ReadOnly = !field.Editable;
                    if (!field.Editable) row.Cells[2].Value = DeadSpaceUi.UiLanguage.Translate("只读");
                }
            }
            finally { populatingSaveGrid = false; }
        }
        private string EditorLabel(SaveValue field)
        {
            string path = field.Path;
            var labels = new Dictionary<string, string> { { "hp", "生命值" }, { "credits", "金钱 / 信用点" }, { "nodes", "能量节点" },
                { "upgrade_level_rig", "护甲等级（RIG）" }, { "upgrade_level_health", "生命升级" }, { "upgrade_level_o2", "氧气升级" },
                { "telekinesis_available", "念力可用" }, { "plasmasaw_available", "切割锯可用" }, { "gameCompleted", "通关标志" }, { "plus", "周目标志" },
                { "available", "持有" }, { "clip", "弹匣 / 能量数值" }, { "altmode", "副模式状态" }, { "current", "当前装备" }, { "unlocked", "已解锁" },
                { "complete", "完成标志" }, { "level", "关卡" }, { "difficulty", "难度" }, { "name", "名称" }, { "level_id", "保存关卡" }, { "savetype", "保存类型" },
                { "attack", "攻击增强" }, { "defense", "防御增强" }, { "loot", "资源增强" }, { "nodes_2", "节点增强" }, { "nodes_1_registration", "注册增强" },
                { "subtitles_enabled", "字幕" }, { "auto_reload", "自动装弹" }, { "tutorials_enabled", "教程提示" }, { "y_flipped", "反转垂直" },
                { "plus_unlocked", "新周目解锁提示" }, { "sfx_volume", "音效音量" }, { "music_volume", "音乐音量" }, { "brightness", "亮度" }, { "sensitivity", "灵敏度" } };
            int last = path.LastIndexOf('/'); string leaf = path.Substring(last + 1), label;
            if (!labels.TryGetValue(leaf, out label)) label = leaf;
            if (path.StartsWith("player/weapons/"))
            {
                string[] parts = path.Split('/');
                var weapons = new Dictionary<string, string> { { "plasmacutter", "等离子切割器" }, { "ripper", "切割锯" }, { "linegun", "线性枪" }, { "corer", "岩石取芯器" }, { "burst_rifle", "大脉冲" }, { "stasis", "静滞" } };
                string weapon; if (!weapons.TryGetValue(parts[2], out weapon)) weapon = parts[2]; return weapon + " · " + label;
            }
            if (path.StartsWith("levelCompletes/") || path.StartsWith("achievementCompletes/"))
            {
                string prefix = path.Substring(0, last); SaveValue title, difficulty;
                if (editorDocument.Values.TryGetValue(prefix + "/level", out title) || editorDocument.Values.TryGetValue(prefix + "/name", out title))
                    return title.Value + (editorDocument.Values.TryGetValue(prefix + "/difficulty", out difficulty) ? " / " + difficulty.Value : "") + " · " + label;
            }
            return last < 0 || path.StartsWith("player/") && last == 6 ? label : path.Substring(0, last) + " · " + label;
        }
        private Dictionary<string, string> EditorChanges()
        {
            if (editorDocument == null) throw new InvalidDataException("请先读取存档");
            saveGrid.EndEdit(); return new Dictionary<string, string>(pendingSaveChanges);
        }
        private void PreviewEditorChanges()
        {
            try { MessageBox.Show(this, editorDocument == null ? "请先读取存档" : editorDocument.Preview(EditorChanges()), "更改预览", MessageBoxButtons.OK, MessageBoxIcon.Information); }
            catch (Exception error) { status.Text = "更改未通过检查：" + error.Message; }
        }
        private void WriteEditorChanges()
        {
            ApplyAllSaveChanges();
        }
        private void ApplyAllSaveChanges()
        {
            try
            {
                SaveStorage.RequireStopped();
                bool attack = attackPlugin.Checked, defense = defensePlugin.Checked, loot = lootPlugin.Checked;
                bool pluginChanges = loadedSave != null && (loadedSave.NeedsInitialization ||
                    attack != loadedSave.Enabled("attack") || defense != loadedSave.Enabled("defense") || loot != loadedSave.Enabled("loot"));
                var backups = new List<string>();
                var changes = editorDocument == null ? new Dictionary<string, string>() : EditorChanges();
                if (editorDocument != null && changes.Count != 0) editorDocument.EditBytes(changes);
                if (pluginChanges) { loadedSave.RequireUnchanged(); loadedSave.EditBytes(attack, defense, loot); }
                if (editorDocument != null && changes.Count != 0)
                {
                    backups.Add(editorDocument.Write(changes));
                    ReadEditorSave();
                    // The shared settings editor above may point at the same file.
                    // Refresh it after the lower editor writes so the optimistic
                    // source-hash check does not treat our own write as a conflict.
                    if (loadedSave != null) ReadCurrentSave();
                }
                if (pluginChanges)
                {
                    backups.Add(loadedSave.Write(attack, defense, loot));
                    ReadCurrentSave();
                }
                if (backups.Count == 0) throw new InvalidDataException("请先读取存档并修改字段或插件勾选");
                status.Text = "已应用全部存档修改并重读；自动备份：" + String.Join("；", backups.ToArray());
                saveStatus.Text = "已应用全部存档修改并重读。自动备份：\r\n" + String.Join("\r\n", backups.ToArray());
            }
            catch (Exception error)
            {
                status.Text = "未应用存档修改：" + error.Message;
                saveStatus.Text = error.Message;
            }
        }
        private void ImportEditorCopy()
        {
            using (var dialog = new OpenFileDialog { Filter = "存档文件 (*.sb)|*.sb", Title = "选择备份中的任一存档；将复制整组，不改原备份" })
            {
                if (dialog.ShowDialog(this) != DialogResult.OK) return;
                try
                {
                    SaveStorage.RequireStopped();
                    string source = Path.GetDirectoryName(dialog.FileName); SaveStorage.RequireNoLinks(source);
                    string target = Path.Combine(SelectedDataRoot(), "editor-imports", Guid.NewGuid().ToString("N"), "var");
                    SaveStorage.RequireNoLinks(target); Directory.CreateDirectory(target);
                    foreach (string file in Directory.GetFiles(source))
                    {
                        string name = Path.GetFileName(file);
                        if (!name.EndsWith(".sb", StringComparison.OrdinalIgnoreCase) && name != "last_version.txt") continue;
                        SaveStorage.RequireNoLinks(file);
                        byte[] bytes = File.ReadAllBytes(file); string destination = Path.Combine(target, name);
                        File.WriteAllBytes(destination, bytes);
                        using (var hash = SHA256.Create())
                        {
                            string expected = Convert.ToBase64String(hash.ComputeHash(bytes));
                            if (Convert.ToBase64String(hash.ComputeHash(File.ReadAllBytes(destination))) != expected || Convert.ToBase64String(hash.ComputeHash(File.ReadAllBytes(file))) != expected)
                                throw new IOException("备份副本校验失败：" + name);
                        }
                    }
                    editorImportedDirectory = target; InvalidateEditor(); ReadEditorSave();
                    status.Text = "正在编辑导入副本；原备份和活动存档均保留。副本不会自动用于游戏。";
                }
                catch (Exception error) { status.Text = "未导入：" + error.Message; }
            }
        }
        private void RestoreEditorBackup()
        {
            using (var dialog = new OpenFileDialog { Filter = "编辑器备份清单|original.sha256", Title = "选择需要恢复的整组备份" })
            {
                if (dialog.ShowDialog(this) != DialogResult.OK) return;
                string destination = Path.GetDirectoryName(EditorFile());
                if (MessageBox.Show(this, "将把备份清单中的全部文件恢复到：\r\n" + destination + "\r\n\r\n会先完整备份当前存档；未列出的文件保留。", "恢复整组存档", MessageBoxButtons.OKCancel, MessageBoxIcon.Question) != DialogResult.OK) return;
                try { string backup = SaveBackup.Restore(dialog.FileName, destination); InvalidateEditor(); ReadEditorSave(); status.Text = "已恢复并校验；恢复前备份：" + backup; }
                catch (Exception error) { status.Text = "恢复失败：" + error.Message; }
            }
        }
    }
}

