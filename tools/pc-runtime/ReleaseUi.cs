using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Net;
using System.Text;
using System.Text.RegularExpressions;
using System.Threading;
using System.Web.Script.Serialization;
using System.Windows.Forms;

namespace DeadSpaceUi
{
    internal static class ReleaseChannel
    {
        internal const string Repository = "SomJsA/HyperHLE_ds_compat-pc";
        internal const string ProjectUrl = "https://github.com/" + Repository;
        internal const string ReleaseUrl = ProjectUrl + "/releases/latest";
        internal const string ApiUrl = "https://api.github.com/repos/" + Repository + "/releases/latest";
        internal const string CurrentTag = "20261112-r4";

        internal sealed class Result { internal string State, Tag; }
        internal static Result Interpret(int code, string json)
        {
            if (code == 401 || code == 403 || code == 404) return new Result { State = "unavailable" };
            if (code != 200) return new Result { State = "network" };
            try {
                var data = new JavaScriptSerializer().DeserializeObject(json) as Dictionary<string, object>;
                object value;
                if (data == null || !data.TryGetValue("tag_name", out value) || !(value is string)) return new Result { State = "invalid" };
                string tag = (string)value;
                if (tag.Length < 1 || tag.Length > 100 || tag.IndexOfAny(new[] {'\r','\n'}) >= 0) return new Result { State = "invalid" };
                if (data.ContainsKey("draft") && Object.Equals(data["draft"], true)) return new Result { State = "invalid" };
                if (String.Equals(tag.TrimStart('v','V'), CurrentTag, StringComparison.OrdinalIgnoreCase)) return new Result { State = "current", Tag = tag };
                var before = Regex.Matches(CurrentTag, @"\d+"); var after = Regex.Matches(tag, @"\d+");
                if (before.Count == after.Count && after.Count > 0 && after[0].Value.Length == before[0].Value.Length) {
                    for (int i = 0; i < before.Count; i++) {
                        long a, b; if (!Int64.TryParse(before[i].Value, out a) || !Int64.TryParse(after[i].Value, out b)) break;
                        if (b > a) return new Result { State = "new", Tag = tag };
                        if (b < a) return new Result { State = "older", Tag = tag };
                    }
                }
                return new Result { State = "available", Tag = tag };
            } catch (ArgumentException) { return new Result { State = "invalid" }; }
        }
        internal static Result Check()
        {
            try {
                ServicePointManager.SecurityProtocol |= (SecurityProtocolType)3072;
                var request = (HttpWebRequest)WebRequest.Create(ApiUrl);
                request.Method = "GET"; request.UserAgent = "DeadSpace-Mobile-PC/" + CurrentTag;
                request.Accept = "application/vnd.github+json"; request.Timeout = 7000; request.ReadWriteTimeout = 7000;
                request.AllowAutoRedirect = false;
                using (var response = (HttpWebResponse)request.GetResponse())
                using (var stream = response.GetResponseStream())
                using (var output = new MemoryStream()) {
                    byte[] buffer = new byte[8192]; int count;
                    while ((count = stream.Read(buffer, 0, buffer.Length)) != 0) {
                        if (output.Length + count > 512 * 1024) return new Result { State = "invalid" };
                        output.Write(buffer, 0, count);
                    }
                    return Interpret((int)response.StatusCode, Encoding.UTF8.GetString(output.ToArray()));
                }
            } catch (WebException error) {
                using (var response = error.Response as HttpWebResponse)
                    return Interpret(response == null ? 0 : (int)response.StatusCode, "");
            } catch (IOException) { return new Result { State = "network" }; }
        }
    }

    internal sealed class ProjectAboutPanel : Panel
    {
        private readonly TableLayoutPanel content = new TableLayoutPanel();
        private readonly List<Action> translations = new List<Action>();
        private readonly Label updateStatus = new Label();
        private readonly Button check = new Button();
        private ReleaseChannel.Result result;
        private bool checking, started;
        internal ProjectAboutPanel()
        {
            Tag = "explicit-bilingual"; AutoScroll = true; Dock = DockStyle.Fill;
            BackColor = Color.FromArgb(19,30,40); ForeColor = Color.FromArgb(221,232,237);
            content.Dock = DockStyle.Top; content.AutoSize = true; content.ColumnCount = 1; content.Padding = new Padding(24);
            content.ColumnStyles.Add(new ColumnStyle(SizeType.Percent,100)); Controls.Add(content);
            Add("00 关于", "00 About", 20, true);
            Add("死亡空间移动版（IOS端高清移植）", "DEAD SPACE MOBILE", 17, true);
            Add("快行动，破坏者！\r\n让我们把泰坦空间站烧成灰！", "Wake the fuck up, Vandal !\r\nWe Have a Titan Station to Burn !", 11, false);
            Add("基于 touchHLE 与 HyperHLE。项目仓库当前可能处于私有状态。", "Based on touchHLE and HyperHLE. The project repository may currently be private.", 10, false);
            Add("SomJs：项目主要开发、手柄游玩测试、骁龙 Android 设备游玩测试\r\n"+
                "lxyun_2（腹黑小喵）：超级校对、超级测试\r\nMisakaCharlotte（是的呀啊）：超级校对、超级测试\r\n"+
                "特洛克·洛克莱斯：超级检查测试\r\n天雷本雷：键鼠游玩测试、天玑 Android 设备游玩测试\r\n豆沙包里有磁：键鼠测试（顶着极低的全转储帧率）",
                "SomJs: lead development, gamepad and Snapdragon Android playtesting\r\n"+
                "lxyun_2 (腹黑小喵): extensive proofreading and testing\r\nMisakaCharlotte (是的呀啊): extensive proofreading and testing\r\n"+
                "特洛克·洛克莱斯: extensive verification and testing\r\n天雷本雷: keyboard/mouse and Dimensity Android playtesting\r\n豆沙包里有磁: keyboard/mouse testing at very low frame rates with full dumps", 10, false);
            AddLink("问题反馈 / Bug reports · DeadSpaceMobile Community", "https://github.com/SomJsA/DeadSpaceMobile-Community/issues");
            AddLink("PC 项目 / PC project", ReleaseChannel.ProjectUrl);
            AddLink("touchHLE", "https://github.com/touchHLE/touchHLE");
            AddLink("HyperHLE", "https://github.com/HyperHLE/HyperHLE");
            AddLink("SomJs · Bilibili", "https://space.bilibili.com/34237026");
            AddLink("SomJs · Reddit", "https://www.reddit.com/user/SomJs/");
            AddLink("SomJs · GitHub", "https://github.com/SomJsA");
            AddLink("死亡空间贴吧汉化组 / Dead Space Tieba translation team", "https://tieba.baidu.com/wxf/341915?kw=%E6%AD%BB%E4%BA%A1%E7%A9%BA%E9%97%B4&fr=sharewise");
            Add("原版 credits_list 制作名单保留。Dead Space 与游戏素材归 Electronic Arts 所有；各组件按各自许可证提供。", "The original credits_list is retained. Dead Space and its game assets belong to Electronic Arts; each component retains its own license.", 9, false);
            Add("检查 GitHub 更新", "GitHub updates", 13, true);
            Add("当前版本："+ReleaseChannel.CurrentTag, "Current version: "+ReleaseChannel.CurrentTag, 10, false);
            updateStatus.AutoSize = true; updateStatus.Dock = DockStyle.Top; updateStatus.Margin = new Padding(0,6,0,8); content.Controls.Add(updateStatus);
            check.AutoSize = true; check.MinimumSize = new Size(140,36); check.FlatStyle = FlatStyle.Flat; check.Click += delegate { CheckUpdates(); }; content.Controls.Add(check);
            AddLink("查看发布页 / Open releases", ReleaseChannel.ReleaseUrl);
            Add("仅提示，不自动下载或安装。私有仓库未授权时会说明无法检查。", "Notifications only; no automatic downloads or installation. Private repositories without access are reported as unavailable.", 9, false);
            content.SizeChanged += delegate { foreach (Control c in content.Controls) if (c is Label) c.MaximumSize = new Size(Math.Max(120,content.ClientSize.Width-content.Padding.Horizontal-8),0); };
            ApplyLanguage();
        }
        private void Add(string zh, string en, float size, bool bold)
        {
            var label = new Label { AutoSize = true, Dock = DockStyle.Top, Font = new Font("Microsoft YaHei UI",size,bold?FontStyle.Bold:FontStyle.Regular), Margin = new Padding(0,0,0,10) };
            translations.Add(delegate { label.Text = UiLanguage.English ? en : zh; }); content.Controls.Add(label);
        }
        private void AddLink(string title, string url)
        {
            var link = new LinkLabel { Text = title, AutoSize = true, Dock = DockStyle.Top, LinkColor = Color.FromArgb(98,218,232), Margin = new Padding(0,0,0,8) };
            link.LinkClicked += delegate { try { System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo { FileName=url, UseShellExecute=true }); } catch (System.ComponentModel.Win32Exception) { } };
            content.Controls.Add(link);
        }
        internal void ApplyLanguage()
        {
            foreach (var apply in translations) apply();
            check.Text = UiLanguage.English ? "Check for updates" : "检查更新";
            string state = checking ? "checking" : result == null ? "idle" : result.State;
            string tag = result == null ? "" : result.Tag;
            var zh = new Dictionary<string,string> { {"checking","正在检查 GitHub…"},{"idle","尚未检查更新。"},{"unavailable","无法检查：仓库未公开、缺少权限或尚无 Release。"},{"network","暂时无法连接 GitHub，请稍后重试。"},{"invalid","发布信息不可用，请打开发布页查看。"},{"current","当前版本与最新 Release 一致。"},{"older","GitHub 最新发布早于当前构建："+tag},{"new","发现更新："+tag+"。可打开发布页查看。"},{"available","发现 GitHub 发布："+tag+"；请在发布页核对版本。"} };
            var en = new Dictionary<string,string> { {"checking","Checking GitHub…"},{"idle","Updates have not been checked."},{"unavailable","Cannot check: repository is private, access is missing, or no Release is published."},{"network","Cannot connect to GitHub. Try again later."},{"invalid","Release details unavailable. Open the releases page."},{"current","This build matches the latest Release."},{"older","The latest GitHub release is older than this build: "+tag},{"new","Update available: "+tag+". Open the releases page for details."},{"available","GitHub release found: "+tag+". Check the version on the releases page."} };
            updateStatus.Text = (UiLanguage.English ? en : zh)[state];
        }
        internal void StartAutomaticCheck()
        {
            if (started || Environment.GetEnvironmentVariable("TOUCHHLE_UPDATE_CHECK")=="0") return;
            started=true; CheckUpdates();
        }
        private void CheckUpdates()
        {
            if (checking) return; checking=true; check.Enabled=false; ApplyLanguage();
            ThreadPool.QueueUserWorkItem(delegate {
                var value=ReleaseChannel.Check();
                if (IsDisposed || !IsHandleCreated) return;
                try { BeginInvoke((Action)delegate { if (IsDisposed) return; result=value; checking=false; check.Enabled=true; ApplyLanguage(); }); }
                catch (InvalidOperationException) { }
            });
        }
    }
}
