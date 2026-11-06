using System;
using System.Collections;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Globalization;
using System.IO;
using System.Reflection;
using System.Security.Cryptography;
using System.Text;
using System.Text.RegularExpressions;
using System.Web.Script.Serialization;
using System.Windows.Forms;

namespace DeadSpaceBootstrap
{
    internal static class StartupReviewData
    {
        internal static string Normalize(string value) { return Regex.Replace((value ?? "").Trim(), @"\s+", " "); }
        internal static object Value(IDictionary<string, object> data, string key)
        { object value; return data != null && data.TryGetValue(key, out value) ? value : null; }
        internal static double Number(IDictionary<string, object> data, string key, double fallback)
        {
            object value = Value(data, key); double result;
            return value != null && Double.TryParse(Convert.ToString(value, CultureInfo.InvariantCulture), NumberStyles.Float,
                CultureInfo.InvariantCulture, out result) && !Double.IsNaN(result) && !Double.IsInfinity(result) ? result : fallback;
        }
        internal static string Text(IDictionary<string, object> data, string key)
        { return Convert.ToString(Value(data, key), CultureInfo.InvariantCulture); }
        internal static Dictionary<string, object> Read(string path)
        {
            try
            {
                var info = new FileInfo(path); if (!info.Exists || info.Length > 65536) return null;
                using (var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete))
                using (var reader = new StreamReader(stream, Encoding.UTF8))
                    return new JavaScriptSerializer { MaxJsonLength = 65536, RecursionLimit = 16 }.DeserializeObject(reader.ReadToEnd()) as Dictionary<string, object>;
            }
            catch (IOException) { return null; } catch (UnauthorizedAccessException) { return null; } catch (ArgumentException) { return null; } catch (InvalidOperationException) { return null; }
        }
        internal static Dictionary<string, object> Known(string cpu, string gpu, double vram)
        {
            try
            {
                using (var stream = Assembly.GetExecutingAssembly().GetManifestResourceStream("DeadSpaceBootstrap.HardwareProfiles.json"))
                {
                    if (stream == null) return null;
                    using (var reader = new StreamReader(stream, Encoding.UTF8))
                    {
                        var root = new JavaScriptSerializer().DeserializeObject(reader.ReadToEnd()) as Dictionary<string, object>;
                        var items = Value(root, "profiles") as object[]; if (items == null) return null;
                        foreach (var item in items)
                        {
                            var p = item as Dictionary<string, object>;
                            if (Text(p, "platform") == "pc" && Matches(p, cpu, gpu, vram)) return p;
                        }
                    }
                }
            }
            catch (Exception) { }
            return null;
        }
        internal static bool Matches(IDictionary<string, object> p, string cpu, string gpu, double vram)
        {
            return String.Equals(Normalize(Text(p, "cpu")), Normalize(cpu), StringComparison.OrdinalIgnoreCase)
                && String.Equals(Normalize(Text(p, "gpu")), Normalize(gpu), StringComparison.OrdinalIgnoreCase)
                && vram >= Number(p, "min_vram_gib", Double.MaxValue);
        }
        internal static bool Current(IDictionary<string, object> data, int pid)
        { return Number(data, "schema", 0) == 1 && Number(data, "pid", 0) == pid; }
        internal static string Features(IDictionary<string, object> data)
        {
            var text = new StringBuilder();
            string renderer = Text(data, "renderer");
            text.AppendLine("渲染器：" + (renderer.Length > 0 ? renderer : "待本次运行确认"));
            double width = Number(data, "width", 0), height = Number(data, "height", 0);
            text.AppendLine("内部画面：" + (width > 0 && height > 0 ? width + " × " + height : "待确认"));
            object patch = Value(data, "text_patch");
            text.AppendLine("中文文本：" + (patch is bool ? ((bool)patch ? "本次已挂载" : "已回退，部分原生文本可能恢复原版") : "待本次挂载确认"));
            text.AppendLine(Filter(data, "af", "AF", "远处斜面纹理清晰度降低"));
            text.AppendLine(Filter(data, "msaa", "MSAA", "边缘抗锯齿减弱"));
            return text.ToString();
        }
        private static string Filter(IDictionary<string, object> data, string key, string name, string impact)
        {
            double requested=Number(data,key+"_requested",-1),actual=Number(data,key+"_actual",-1);
            if (requested < 0 || actual < 0) return name + "：待实际验证";
            if (requested <= 1) return name + "：未开启";
            if (actual < requested) return name + "：请求"+requested+"× → 实际"+actual+"×，"+impact;
            if (key=="af" && !(Value(data,"af_verified") is bool && (bool)Value(data,"af_verified"))) return "AF：读回尚未确认";
            return name+"：实际"+actual+"×，增加GPU负载";
        }
    }

    internal sealed class StartupReviewWindow : Form
    {
        private readonly Label detail;
        private readonly Timer timer = new Timer();
        private readonly string path, advice, expectedGpu;
        private readonly int pid;
        private readonly DateTime born=DateTime.UtcNow;
        private DateTime ready;
        protected override bool ShowWithoutActivation { get { return true; } }
        internal StartupReviewWindow(string path, int pid, string advice, string texture, string expectedGpu)
        {
            this.path=path;this.pid=pid;this.advice=advice;this.expectedGpu=expectedGpu;
            Text="启动自检";ClientSize=new Size(680,490);StartPosition=FormStartPosition.CenterParent;
            Font=new Font("Microsoft YaHei UI",11f);BackColor=Color.FromArgb(9,18,25);ForeColor=Color.Gainsboro;
            FormBorderStyle=FormBorderStyle.FixedToolWindow;ShowInTaskbar=false;
            Controls.Add(new Label{Text="启动自检",Font=new Font(Font.FontFamily,19f,FontStyle.Bold),ForeColor=Color.FromArgb(94,218,230),Location=new Point(24,20),AutoSize=true});
            detail=new Label{Location=new Point(26,70),Size=new Size(628,310),AutoEllipsis=true};Controls.Add(detail);
            var close=new Button{Text="进入游戏",Location=new Point(520,440),Size=new Size(130,32)};close.Click+=delegate{Close();};Controls.Add(close);
            Controls.Add(new Label{Text=String.IsNullOrEmpty(texture)?"高清资源：未指定资源包":"高清资源目录已选择，关卡实际上传待确认",Location=new Point(26,390),Size=new Size(620,43)});
            timer.Interval=350;timer.Tick+=delegate{UpdateReport();};FormClosed+=delegate{timer.Stop();timer.Dispose();};timer.Start();UpdateReport();
        }
        private void UpdateReport()
        {
            var data=StartupReviewData.Read(path);if(!StartupReviewData.Current(data,pid))data=null;
            string renderer=StartupReviewData.Text(data,"renderer");
            bool mismatch=renderer.Length>0 && !String.IsNullOrEmpty(expectedGpu) && renderer.IndexOf(expectedGpu,StringComparison.OrdinalIgnoreCase)<0;
            string diagnostic="自动诊断：未请求或尚未建立会话";
            if(!String.IsNullOrEmpty(path)) {
                string shutdown=DiagnosticFiles.ShutdownStatus(Path.GetDirectoryName(path));
                var health=StartupReviewData.Read(Path.Combine(Path.GetDirectoryName(path),"health.json"));
                if(shutdown!=null)diagnostic="自动"+shutdown;
                else if(health!=null){int level=StartupReviewData.Text(health,"writer_error").Length>0?3:(int)StartupReviewData.Number(health,"detail_level",3);diagnostic="自动诊断："+(level==0?(StartupReviewData.Number(health,"persisted_frame_records",0)>0 && StartupReviewData.Number(health,"persisted_event_records",0)>0?"帧与事件落盘已核验":"等待有效帧/事件落盘确认"):level==1?"过载降频，缺口已标注":level==2?"详细记录暂停，保留轻量计数":"写入失败，已有记录保留");}
                else if(File.Exists(Path.Combine(Path.GetDirectoryName(path),"host.json")))diagnostic="自动诊断：等待后端确认";
            }
            detail.Text=StartupReviewData.Features(data)+diagnostic+"\n\n"+(mismatch?"当前渲染器与识别显卡不同，请核对系统显卡选择；已保存设置保持不变。":advice);
            bool complete=data!=null && StartupReviewData.Text(data,"renderer").Length>0
                && StartupReviewData.Number(data,"msaa_actual",-1)>=0 && StartupReviewData.Number(data,"af_actual",-1)>=0
                && StartupReviewData.Value(data,"text_patch") is bool;
            if(complete && ready==default(DateTime))ready=DateTime.UtcNow;
            if((complete && (DateTime.UtcNow-ready).TotalSeconds>=3 && (DateTime.UtcNow-born).TotalSeconds>=4)
                ||(DateTime.UtcNow-born).TotalSeconds>=30)Close();
        }
    }

    internal sealed partial class LauncherForm
    {
        private string startupReportPath;
        private static bool ReviewProcessAlive(Process p)
        {try{return p!=null&&!p.HasExited;}catch(InvalidOperationException){return false;}}
        private string HardwareAdviceText()
        {return "首次按显示器预置并保存；后续读取配置。内部尺寸与输出尺寸独立，画质调整保留分辨率。";}
        private void AddHardwareReviewButton(Control page)
        {
            var button=new Button{Text="自动配置与启动状态",Location=new Point(492,23),Size=new Size(190,32)};
            button.Click+=delegate{
                string message=detectedHardware.GpuName+"\n"+detectedHardware.CpuName+"\n\n"+HardwareAdviceText()+"\n\n"+renderResolutionSummary.Text;
                var data=String.IsNullOrEmpty(startupReportPath)?null:StartupReviewData.Read(startupReportPath);
                if(data!=null)message+="\n\n最近一次运行\n"+StartupReviewData.Features(data);
                MessageBox.Show(this,message,"自动配置与启动状态",MessageBoxButtons.OK,MessageBoxIcon.Information);
            };page.Controls.Add(button);
        }
        private void PrepareStartupReview(ProcessStartInfo info)
        {
            startupReportPath=null;
            info.EnvironmentVariables.Remove("TOUCHHLE_DS_STARTUP_REPORT");
            info.EnvironmentVariables["TOUCHHLE_DS_BENCHMARK"]="0";
            try
            {
                string root=Path.Combine(baseDir,"diagnostics");SaveStorage.RequireNoLinks(root);Directory.CreateDirectory(root);
                startupReportPath=Path.Combine(root,"startup-"+Guid.NewGuid().ToString("N")+".json");
                info.EnvironmentVariables["TOUCHHLE_DS_STARTUP_REPORT"]=startupReportPath;
            }
            catch(IOException){}
            catch(UnauthorizedAccessException){}
        }
        private void ShowStartupReview(string texture)
        {
            var process=gameProcess;if(process==null)return;
            var wait=new Timer{Interval=250};int ticks=0;
            wait.Tick+=delegate{
                if(!ReviewProcessAlive(process) || ++ticks>120){wait.Stop();wait.Dispose();return;}
                IntPtr handle;try{process.Refresh();handle=process.MainWindowHandle;}catch(InvalidOperationException){wait.Stop();wait.Dispose();return;}if(handle==IntPtr.Zero)return;
                wait.Stop();wait.Dispose();var view=new StartupReviewWindow(startupReportPath,process.Id,HardwareAdviceText(),texture,detectedHardware.GpuName);
                view.Show(new NativeReviewOwner(handle));
            };wait.Start();
        }
        private sealed class NativeReviewOwner:IWin32Window
        {private readonly IntPtr handle;internal NativeReviewOwner(IntPtr value){handle=value;}public IntPtr Handle{get{return handle;}}}
    }
}
