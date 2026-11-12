using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.IO.Compression;
using System.Globalization;
using System.Security.Cryptography;
using System.Text;
using System.Threading.Tasks;
using System.Web.Script.Serialization;
using System.Windows.Forms;

namespace DeadSpaceBootstrap
{
    internal static class DiagnosticFiles
    {
        internal static Dictionary<string,object> Read(string path){return StartupReviewData.Read(path);}
        internal static void WriteNew(string path,object value)
        {
            SaveStorage.RequireNoLinks(path);
            using(var stream=new FileStream(path,FileMode.CreateNew,FileAccess.Write,FileShare.Read))
            using(var writer=new StreamWriter(stream,new UTF8Encoding(false)))writer.WriteLine(new JavaScriptSerializer{MaxJsonLength=16*1024*1024,RecursionLimit=32}.Serialize(value));
        }
        internal static string Root(string dataRoot){return Path.Combine(dataRoot,"diagnostics","play-sessions");}
        internal static IEnumerable<string> Sessions(string dataRoot)
        {
            string root=Root(dataRoot);SaveStorage.RequireNoLinks(root);if(!Directory.Exists(root))yield break;
            foreach(string path in Directory.EnumerateDirectories(root,"session-*",SearchOption.TopDirectoryOnly))
            {
                SaveStorage.RequireNoLinks(path);var host=Read(Path.Combine(path,"host.json"));
                if(StartupReviewData.Number(host,"schema",0)==1 && StartupReviewData.Text(host,"kind")=="deadspace-play-diagnostics")yield return path;
            }
        }
        internal static bool NormalEnd(string directory)
        {return StartupReviewData.Value(Read(Path.Combine(directory,"session-end.json")),"normal_exit_observed") as bool? == true;}
        internal static string ShutdownStatus(string directory)
        {
            var end=Read(Path.Combine(directory,"session-end.json"));if(end==null)return null;
            if(StartupReviewData.Text(end,"writer_error").Length>0)return "诊断已停止：写入失败，已有记录保留。";
            double tail=StartupReviewData.Number(end,"queued_tail_not_persisted",-1);
            double inflight=StartupReviewData.Number(end,"shutdown_senders_inflight",-1);
            bool timeout=StartupReviewData.Value(end,"shutdown_drain_timed_out") as bool? == true;
            bool? drained=StartupReviewData.Value(end,"shutdown_queue_drained") as bool?;
            if(tail>0 || inflight>0 || timeout || drained==false)
                return "诊断已结束：存在收尾缺口"+(timeout?"（排空超时）":"")+"；已保留可用数据，请连同结束信息导出。";
            if(StartupReviewData.Value(end,"normal_exit_observed") as bool? != true)
                return "诊断已结束：未观察到正常游戏退出；原因未确认，已有记录保留。";
            if(drained==true && tail==0 && inflight==0)
                return "诊断已结束：已接收队列已排空，最终写入成功。";
            return "诊断已结束：旧记录未提供完整的队尾核验信息。";
        }
        internal static void InspectPrevious(string dataRoot)
        {
            // Caller has established no game is running. Missing normal end is
            // kept distinct from a proven crash; no session or file is deleted.
            foreach(string session in Sessions(dataRoot))
            {
                string marker=Path.Combine(session,"previous-not-normal.json");
                if(!NormalEnd(session)&&!File.Exists(marker))
                    WriteNew(marker,new {schema=1,observed_wall_utc=DateTime.UtcNow.ToString("o"),classification="normal_end_not_observed",reason="May be force close, OS reclaim, power loss or failure; not proof of a crash"});
            }
        }
        internal static void Mark(string session,string note)
        {
            SaveStorage.RequireNoLinks(session);string requests=Path.Combine(session,"requests");SaveStorage.RequireNoLinks(requests);Directory.CreateDirectory(requests);
            WriteNew(Path.Combine(requests,Guid.NewGuid().ToString("N")+".request.json"),new {schema=1,wall_unix_ms=DateTimeOffset.UtcNow.ToUnixTimeMilliseconds(),note=(note??"").Length>500?note.Substring(0,500):(note??"")});
        }
        private static List<string> LastParts(string session,string prefix)
        {
            var latest=new SortedDictionary<long,string>();
            foreach(string file in Directory.EnumerateFiles(session,prefix+"-*",SearchOption.TopDirectoryOnly))
            {
                long part;if(!Int64.TryParse(Path.GetFileNameWithoutExtension(file).Substring(prefix.Length+1),out part))continue;
                latest[part]=file;if(latest.Count>2){var first=latest.GetEnumerator();first.MoveNext();latest.Remove(first.Current.Key);}
            }
            return new List<string>(latest.Values);
        }
        private static IEnumerable<string> BoundedLines(string file)
        {
            SaveStorage.RequireNoLinks(file);using(var reader=new StreamReader(file,Encoding.UTF8))
            {
                var line=new StringBuilder();bool oversized=false;int ch;
                while((ch=reader.Read())>=0)
                {
                    if(ch=='\n'){if(!oversized)yield return line.ToString().TrimEnd('\r');line.Length=0;oversized=false;}
                    else if(line.Length<131072)line.Append((char)ch);else oversized=true;
                }
                if(line.Length>0&&!oversized)yield return line.ToString();
            }
        }
        internal static void RecoverUnexpected(string session)
        {
            string target=Path.Combine(session,"recovered-last30s.json");if(NormalEnd(session)||File.Exists(target))return;
            // Recovery happens during explicit, stopped-game export, not while
            // playing. Work from the latest persisted data, never pretend to
            // know the exact time/cause of a force-killed process.
            var rows=new Queue<long[]>();long last=0,evicted=0;
            foreach(string file in LastParts(session,"frames"))foreach(string line in BoundedLines(file))
            {
                string[] fields=line.Split(',');if(fields.Length!=6)continue;var row=new long[6];bool valid=true;
                for(int i=0;i<6;i++)if(!Int64.TryParse(fields[i],NumberStyles.None,CultureInfo.InvariantCulture,out row[i])){valid=false;break;}
                if(!valid||row[0]<last)continue;last=row[0];while(rows.Count>0&&(rows.Peek()[0]<last-30000000||rows.Count>=65536)){rows.Dequeue();evicted++;}rows.Enqueue(row);
            }
            var events=new Queue<object>();long droppedEvents=0;
            foreach(string file in LastParts(session,"timeline"))foreach(string line in BoundedLines(file))
            {
                try{var data=new JavaScriptSerializer{MaxJsonLength=131072,RecursionLimit=24}.DeserializeObject(line) as Dictionary<string,object>;
                    double at=StartupReviewData.Number(data,"mono_us",-1);if(at<Math.Max(0,last-30000000)||at>last)continue;
                    if(events.Count==4096){events.Dequeue();droppedEvents++;}events.Enqueue(data);}
                catch(ArgumentException){}catch(InvalidOperationException){}
            }
            WriteNew(target,new {schema=1,type="recovered_persisted_tail",reference="latest persisted native frame; exact fault time/cause unknown",last_persisted_mono_us=last,
                requested_from_us=Math.Max(0,last-30000000),frames=rows.ToArray(),frame_columns=new[]{"mono_us","native_present","interval_us","phase","native_state","native_hud"},events=events.ToArray(),
                coverage=new {frames_pruned_or_evicted=evicted,event_evictions=droppedEvents,live_memory_tail_unavailable=true,normal_end_observed=false},
                note="Not proof of a crash. Process termination or disk failure can lose unwritten tail; full original journals remain in this ZIP."});
        }
        internal static bool Allowed(string relative)
        {
            string name=Path.GetFileName(relative);
            if(relative.IndexOfAny(new[]{'\\','/'})>=0)
            {
                string[] parts=relative.Replace('\\','/').Split('/');
                return parts.Length==3 && parts[0]=="requests" && parts[1]=="handled" && parts[2]!= ".." && name.EndsWith(".request.json",StringComparison.Ordinal);
            }
            return name=="host.json"||name=="native-process.json"||name=="session-start.json"||name=="session-end.json"||name=="launcher-exit.json"||name=="health.json"||name=="startup.json"||name=="previous-not-normal.json"||name=="recovered-last30s.json"||name=="diagnostic-map.json"
                ||(name.StartsWith("timeline-",StringComparison.Ordinal)&&name.EndsWith(".jsonl",StringComparison.Ordinal))
                ||(name.StartsWith("frames-",StringComparison.Ordinal)&&name.EndsWith(".csv",StringComparison.Ordinal))
                ||(name.StartsWith("incident-",StringComparison.Ordinal)&&name.EndsWith(".json",StringComparison.Ordinal))
                ||(name.StartsWith("issue-",StringComparison.Ordinal)&&(name.EndsWith(".json",StringComparison.Ordinal)||name.EndsWith(".png",StringComparison.Ordinal)));
        }
        internal static void Export(string[] selected,string destination,string saveAttachment,string imageAttachment)
        {
            if(selected.Length==0)throw new IOException("请至少选一个会话。");
            SaveStorage.RequireNoLinks(destination);if(File.Exists(destination))throw new IOException("目标ZIP已存在，请选择新文件名。");
            string temporary=destination+"."+Guid.NewGuid().ToString("N")+".tmp";
            var usedFolders=new HashSet<string>(StringComparer.OrdinalIgnoreCase);
            var sessionFolders=new List<object>();
            try
            {
                using(var stream=new FileStream(temporary,FileMode.CreateNew,FileAccess.Write,FileShare.None))
                using(var zip=new ZipArchive(stream,ZipArchiveMode.Create))
                {
                    foreach(string session in selected)
                    {
                        SaveStorage.RequireNoLinks(session);var host=Read(Path.Combine(session,"host.json"));
                        if(StartupReviewData.Text(host,"kind")!="deadspace-play-diagnostics")throw new IOException("会话身份无法确认，原文件保留。");
                        string leaf=Path.GetFileName(session),folder=leaf;int suffix=2;
                        if(String.IsNullOrEmpty(leaf)||leaf=="."||leaf=="..")throw new IOException("会话目录名称无效，原文件保留。");
                        while(!usedFolders.Add(folder))folder=leaf+"-"+(suffix++).ToString(CultureInfo.InvariantCulture);
                        sessionFolders.Add(new {source_leaf=leaf,archive_folder=folder});
                        RecoverUnexpected(session);
                        var files=new List<string>(Directory.EnumerateFiles(session,"*",SearchOption.TopDirectoryOnly));
                        string handled=Path.Combine(session,"requests","handled");SaveStorage.RequireNoLinks(handled);
                        if(Directory.Exists(handled))files.AddRange(Directory.EnumerateFiles(handled,"*.request.json",SearchOption.TopDirectoryOnly));
                        foreach(string file in files)
                        {
                            SaveStorage.RequireNoLinks(file);string relative=file.Substring(session.Length+1).Replace('\\','/');if(!Allowed(relative))continue;
                            var entry=zip.CreateEntry(folder+"/"+relative,CompressionLevel.Fastest);
                            using(var input=new FileStream(file,FileMode.Open,FileAccess.Read,FileShare.Read))using(var output=entry.Open())input.CopyTo(output);
                        }
                    }
                    foreach(var attachment in new[]{new KeyValuePair<string,string>("attachments/three-slot-saves.zip",saveAttachment),
                        new KeyValuePair<string,string>("attachments/user-selected-image"+(String.IsNullOrEmpty(imageAttachment)?"":Path.GetExtension(imageAttachment).ToLowerInvariant()),imageAttachment)})
                    {
                        if(String.IsNullOrEmpty(attachment.Value))continue;SaveStorage.RequireNoLinks(attachment.Value);
                        var entry=zip.CreateEntry(attachment.Key,CompressionLevel.Fastest);
                        using(var input=File.OpenRead(attachment.Value))using(var output=entry.Open())input.CopyTo(output);
                    }
                    var identity=zip.CreateEntry("export-manifest.json");using(var writer=new StreamWriter(identity.Open(),new UTF8Encoding(false)))
                        writer.Write(new JavaScriptSerializer().Serialize(new {schema=1,sessions=selected.Length,session_folders=sessionFolders,saves_explicitly_selected=!String.IsNullOrEmpty(saveAttachment),image_explicitly_selected=!String.IsNullOrEmpty(imageAttachment),automatic_upload=false}));
                    var readme=zip.CreateEntry("诊断说明.txt");using(var writer=new StreamWriter(readme.Open(),new UTF8Encoding(false)))
                        writer.Write("关键事件、帧时间和诊断开销以session-start.json单调时间轴关联。默认无截图、录屏、逐键输入或存档。\n未正常结束不等于崩溃；缺口不等于没有卡顿；写入活动和长帧重合不能自动证明因果。\nincident文件含异常前约30秒和后约3秒可用数据；不足30秒、降频、暂停或终止均会标注覆盖范围。原始记录无固定总容量限制、不会自动删除。\n");
                }
                File.Move(temporary,destination);
            }
            finally{if(File.Exists(temporary))File.Delete(temporary);}
        }
    }

    internal sealed partial class LauncherForm
    {
        private readonly CheckBox playDiagnostics=new CheckBox{Checked=false};
        private readonly ComboBox diagnosticDetail=new NoWheelComboBox{DropDownStyle=ComboBoxStyle.DropDownList};
        private readonly Label diagnosticStatus=new DeadSpaceUi.UiLanguage.LocalizedLabel();
        private readonly TextBox problemNote=new TextBox{MaxLength=500};
        private string diagnosticSession;
        private bool diagnosticExporting;
        private void BuildPlayDiagnostics(Control page)
        {
            // Keep the existing navigation and advanced grouping. Diagnostics
            // are visible first; the existing advanced controls remain below.
            Panel outer=(Panel)page;
            var diagnosticPanel=new Panel{Location=new Point(0,850),Size=new Size(710,420)};outer.Controls.Add(diagnosticPanel);page=diagnosticPanel;
            AddCheck(page,playDiagnostics,0,"自动诊断（默认关闭；手动开启=按档位全量采集）",false);
            LabelText(page,"关闭时完全关闭持续诊断、场景追踪和卡顿探针；F7 仍可单次保存问题截图和信息。\r\n开启后按采集档位记录前后时序；不录屏、不记逐键输入、不自动上传。",30,40,670,48);
            problemNote.SetBounds(30,95,385,30);page.Controls.Add(problemNote);
            var mark=new Button{Text="自动诊断：标记前30秒",Location=new Point(430,92),Size=new Size(245,36)};page.Controls.Add(mark);
            mark.Click+=delegate{
                try{if(String.IsNullOrEmpty(diagnosticSession)||!ReviewProcessAlive(gameProcess))throw new IOException("此按钮需要自动诊断；游戏内 F7 可独立保存。");
                    DiagnosticFiles.Mark(diagnosticSession,problemNote.Text);status.Text="已请求保存问题前30秒和后约3秒可用数据；记录异步完成。";}
                catch(Exception error){status.Text="未标记："+error.Message;}
            };
            var export=new Button{Text="导出诊断 ZIP…",Location=new Point(30,137),Size=new Size(200,36)};page.Controls.Add(export);export.Click+=delegate{ExportPlayDiagnostics();};
            var details=new Button{Text="查看本次诊断状态",Location=new Point(248,137),Size=new Size(230,36)};page.Controls.Add(details);details.Click+=delegate{UpdateDiagnosticStatus();MessageBox.Show(this,diagnosticStatus.Text,"诊断状态",MessageBoxButtons.OK,MessageBoxIcon.Information);};
            var folder=new Button{Text="打开诊断文件夹",Location=new Point(496,137),Size=new Size(179,36)};page.Controls.Add(folder);
            folder.Click+=delegate{
                try{string root=DiagnosticFiles.Root(baseDir);SaveStorage.RequireNoLinks(root);
                    if(!Directory.Exists(root))throw new IOException("尚无记录；可在游戏内按 F7 保存，或开启自动诊断。");
                    Process.Start(new ProcessStartInfo{FileName=root,UseShellExecute=true});
                    status.Text="交回此 play-sessions 文件夹中的全部子目录和文件，或退出游戏后导出全部会话 ZIP。";}
                catch(Exception error){status.Text="无法打开诊断目录："+error.Message;}
            };
            diagnosticStatus.SetBounds(30,185,665,65);diagnosticStatus.Text="自动诊断默认完全关闭；勾选后手动开启，选择“超级”即全量档位；F7 始终可单次保存。";page.Controls.Add(diagnosticStatus);
            LabelText(page,"采集档位",30,260,160,25);
            diagnosticDetail.Items.AddRange(new object[]{"基础：帧时间 / 原生状态","增强：全程资源 / 渲染 / JIT 活动","超级：全程活动 + 慢调用定位（开销较高）"});
            diagnosticDetail.SetBounds(200,256,470,36);diagnosticDetail.SelectedIndex=2;page.Controls.Add(diagnosticDetail);
            LabelText(page,"采集档位仅用于自动诊断；F7 单次保存不会启用持续采集。",30,304,650,42);
            // Leave room for both wrapped description lines at the system DPI.
            foreach(Control control in page.Controls) if(control.Top>=92) control.Top+=32;
            int bottom=0;foreach(Control control in outer.Controls)if(!Object.Equals(control.Tag,"advanced-content"))bottom=Math.Max(bottom,control.Bottom);outer.AutoScrollMinSize=new Size(710,bottom+16);
        }
        private static object CopyDiagnosticMap(string basePath,string session,Dictionary<string,object> manifest)
        {
            string expected=StartupReviewData.Text(manifest,"diagnostic_map_sha256"),source=Path.Combine(basePath,"diagnostic-map.json");
            try{
                SaveStorage.RequireNoLinks(source);
                if(expected.Length!=64 || !File.Exists(source) || new FileInfo(source).Length>4*1024*1024)return new {copied=false,reason="missing_or_unbound_map"};
                string actual;using(var sha=SHA256.Create())using(var file=File.OpenRead(source))actual=BitConverter.ToString(sha.ComputeHash(file)).Replace("-","").ToLowerInvariant();
                if(!String.Equals(expected,actual,StringComparison.OrdinalIgnoreCase))return new {copied=false,reason="map_sha_mismatch"};
                File.Copy(source,Path.Combine(session,"diagnostic-map.json"),false);return new {copied=true,sha256=actual};
            }catch(IOException){return new {copied=false,reason="map_unavailable"};}catch(UnauthorizedAccessException){return new {copied=false,reason="map_permission"};}
        }
        private void PreparePlayDiagnostics(ProcessStartInfo info)
        {
            info.EnvironmentVariables["TOUCHHLE_DS_SCENE_TRACE"]="0";info.EnvironmentVariables["TOUCHHLE_DS_STUTTER_PROBE"]="0";
            info.EnvironmentVariables["TOUCHHLE_DS_DIAGNOSTICS"]="0";info.EnvironmentVariables.Remove("TOUCHHLE_DS_DIAGNOSTICS_DIR");diagnosticSession=null;
            string manualRoot=DiagnosticFiles.Root(baseDir);SaveStorage.RequireNoLinks(manualRoot);
            info.EnvironmentVariables["TOUCHHLE_DS_MANUAL_DIAGNOSTICS_ROOT"]=manualRoot;
            if(!playDiagnostics.Checked){diagnosticStatus.Text="自动诊断已关闭；F7 可单次保存。未主动采集前不提供前30秒历史。";return;}
            try
            {
                string dataRoot=baseDir;DiagnosticFiles.InspectPrevious(dataRoot);
                string root=DiagnosticFiles.Root(dataRoot);SaveStorage.RequireNoLinks(root);Directory.CreateDirectory(root);
                string session=Path.Combine(root,"session-"+DateTime.UtcNow.ToString("yyyyMMdd-HHmmss")+"-"+Guid.NewGuid().ToString("N").Substring(0,12));Directory.CreateDirectory(session);
                var manifest=StartupReviewData.Read(Path.Combine(DistributionLayout.Game(baseDir),"build-info.json"));
                object diagnosticMap=CopyDiagnosticMap(DistributionLayout.Game(baseDir),session,manifest);
                var resources=new Dictionary<string,object>();var sources=StartupReviewData.Value(manifest,"resource_sha256") as Dictionary<string,object>;
                foreach(string name in new[]{"res/overlay/deadspace_iphone.bin","src/ds_overlay_default.json","res/overlay/ChapterChinese.ttf","res/overlay/ChapterEnglish.ttf"})resources[name]=StartupReviewData.Value(sources,name);
                string texture=SelectedTextureRoot();string packHash=null;
                if(!String.IsNullOrEmpty(texture)&&File.Exists(Path.Combine(texture,"PACK.json")))using(var sha=SHA256.Create())using(var file=File.OpenRead(Path.Combine(texture,"PACK.json")))packHash=BitConverter.ToString(sha.ComputeHash(file)).Replace("-","").ToLowerInvariant();
                DiagnosticFiles.WriteNew(Path.Combine(session,"host.json"),new {schema=1,kind="deadspace-play-diagnostics",session_id=Path.GetFileName(session),wall_utc=DateTime.UtcNow.ToString("o"),
                    diagnostic_map=diagnosticMap,diagnostic_detail=diagnosticDetail.SelectedIndex,diagnostic_profile=diagnosticDetail.SelectedIndex==2?"super_full_chapter":diagnosticDetail.SelectedIndex==1?"enhanced_full_chapter":"basic",
                    implementation_commit=StartupReviewData.Text(manifest,"implementation_commit"),distribution_id=StartupReviewData.Text(manifest,"distribution_id"),cpu=detectedHardware.CpuName,gpu=detectedHardware.GpuName,
                    dedicated_gpu_bytes=detectedHardware.DedicatedBytes,os=Environment.OSVersion.ToString(),cpu_logical_count=Environment.ProcessorCount,resources=resources,
                    requested=new {render_scale=SelectedRenderScale(),render_resolution=RenderResolutionDiagnostic(),output=SelectedOutputSize()??"internal",fps_cap=Convert.ToString(fpsCap.SelectedItem),msaa=Convert.ToString(msaaSamples.SelectedItem),anisotropy=Convert.ToString(anisotropy.SelectedItem),
                        texture_scale=hdTextures.Checked?(texture6x.Checked?6:texture4x.Checked?4:2):1,texture_pack_manifest_sha256=packHash,fsr=Convert.ToString(fsrMode.SelectedItem),gpu_copy=gpuPresentCopy.Checked,gpu_text=gpuTextOverlay.Checked},
                    resources_identity_scope="packaged fingerprints; native mounted text fingerprint recorded by runtime; unknown resource acceptance remains unknown",
                    privacy=new {automatic_screenshots=false,explicit_F7_game_frame=true,recording=false,keystrokes=false,saves=false,automatic_upload=false,full_personal_paths=false}});
                diagnosticSession=session;startupReportPath=Path.Combine(session,"startup.json");
                info.EnvironmentVariables["TOUCHHLE_DS_STARTUP_REPORT"]=startupReportPath;
                info.EnvironmentVariables["TOUCHHLE_DS_DIAGNOSTICS_DIR"]=session;info.EnvironmentVariables["TOUCHHLE_DS_DIAGNOSTICS"]="1";
                info.EnvironmentVariables["TOUCHHLE_DS_SCENE_TRACE"]=diagnosticDetail.SelectedIndex>=1?"1":"0";
                info.EnvironmentVariables["TOUCHHLE_DS_STUTTER_PROBE"]=diagnosticDetail.SelectedIndex>=2?"1":"0";
            }
            catch(IOException){diagnosticSession=null;diagnosticStatus.Text="诊断目录不可写，本次游戏继续；没有成功采集不代表没有故障。";}
            catch(UnauthorizedAccessException){diagnosticSession=null;diagnosticStatus.Text="诊断目录无权限，本次游戏继续；已有记录保留。";}
        }
        private void RecordDiagnosticProcess()
        {
            if(String.IsNullOrEmpty(diagnosticSession)||gameProcess==null)return;
            try{DiagnosticFiles.WriteNew(Path.Combine(diagnosticSession,"native-process.json"),new {schema=1,pid=gameProcess.Id,start_wall_utc=DateTime.UtcNow.ToString("o")});}catch(IOException){}catch(UnauthorizedAccessException){}
        }
        private void RecordDiagnosticExit(Process process)
        {
            if(String.IsNullOrEmpty(diagnosticSession))return;
            try{string path=Path.Combine(diagnosticSession,"launcher-exit.json");if(!File.Exists(path))DiagnosticFiles.WriteNew(path,new {schema=1,observed_wall_utc=DateTime.UtcNow.ToString("o"),exit_code=process.ExitCode,
                normal_end_observed=DiagnosticFiles.NormalEnd(diagnosticSession),source="owned child process ExitCode and native end marker",classification=DiagnosticFiles.NormalEnd(diagnosticSession)?"normal_end_observed":"normal_end_not_observed; cause not proven"});}
            catch(IOException){}catch(UnauthorizedAccessException){}catch(InvalidOperationException){}
        }
        private void UpdateDiagnosticStatus()
        {
            if(String.IsNullOrEmpty(diagnosticSession))return;
            string shutdown=DiagnosticFiles.ShutdownStatus(diagnosticSession);
            if(shutdown!=null){diagnosticStatus.Text="本次"+shutdown;return;}
            var health=DiagnosticFiles.Read(Path.Combine(diagnosticSession,"health.json"));
            if(health==null){diagnosticStatus.Text="诊断后端尚未就绪或不可用；请检查本次导出中的启动信息。";return;}
            if(ReviewProcessAlive(gameProcess) && (DateTime.UtcNow-File.GetLastWriteTimeUtc(Path.Combine(diagnosticSession,"health.json"))).TotalSeconds>6){diagnosticStatus.Text="诊断健康信息停止更新，当前记录状态无法确认；已落盘数据保留。";return;}
            string error=StartupReviewData.Text(health,"writer_error");
            int level=error.Length>0?3:(int)StartupReviewData.Number(health,"detail_level",3);
            string mode=level==0?"逐帧记录":level==1?"已降频（每4帧保留1帧）":level==2?"详细记录暂停，保留轻量计数":"写入失败，已有记录保留";
            if(error=="low_disk_space")mode="剩余空间不足，已停止详细写入，保留已有记录";
            double persistedFrames=StartupReviewData.Number(health,"persisted_frame_records",0);
            double persistedEvents=StartupReviewData.Number(health,"persisted_event_records",0);
            if(level<3 && (persistedFrames<=0 || persistedEvents<=0))mode="等待有效帧/事件实际落盘确认（不能视为正在记录）";
            diagnosticStatus.Text="本次诊断："+mode+"；已落盘帧 "+persistedFrames+" / 事件 "+persistedEvents+"；丢弃 "+StartupReviewData.Number(health,"dropped_total",0)+"，主动跳过 "+StartupReviewData.Number(health,"detail_skipped_total",0)+"。\r\n当前档位："+(diagnosticDetail.SelectedIndex==2?"超级":diagnosticDetail.SelectedIndex==1?"增强":"基础")+"；前30秒可用关键数据及缺口均保留。";
        }
        private IEnumerable<string> DiagnosticDataRoots()
        {
            var seen=new HashSet<string>(StringComparer.OrdinalIgnoreCase);
            foreach(string root in new[]{baseDir,SelectedDataRoot(),Path.Combine(documentsDirectory,"DeadSpace IOS移植")})
                if(seen.Add(Path.GetFullPath(root)))yield return root;
        }
        private List<string> AllDiagnosticSessions()
        {
            var result=new List<string>();var seen=new HashSet<string>(StringComparer.OrdinalIgnoreCase);
            foreach(string root in DiagnosticDataRoots())foreach(string session in DiagnosticFiles.Sessions(root))if(seen.Add(Path.GetFullPath(session)))result.Add(session);
            return result;
        }
        private async void ExportPlayDiagnostics()
        {
            if(diagnosticExporting)return;
            try
            {
                SaveStorage.RequireStopped();var sessions=AllDiagnosticSessions();sessions.Sort(StringComparer.Ordinal);sessions.Reverse();
                if(sessions.Count==0)throw new IOException("尚无可导出的诊断会话。");
                string[] selected;bool includeSaves,includeImage;
                using(var dialog=new Form{Text="导出完整流程：默认包含全部会话",ClientSize=new Size(700,440),StartPosition=FormStartPosition.CenterParent,MinimizeBox=false,MaximizeBox=false,FormBorderStyle=FormBorderStyle.FixedDialog})
                {
                    var list=new CheckedListBox{Location=new Point(18,18),Size=new Size(660,325),CheckOnClick=true};dialog.Controls.Add(list);
                    foreach(string session in sessions)list.Items.Add(Path.GetFileName(session)+(DiagnosticFiles.NormalEnd(session)?" · 正常结束":" · 未观察到正常结束"));for(int i=0;i<list.Items.Count;i++)list.SetItemChecked(i,true);
                    var saves=new CheckBox{Text="附上当前三个槽位存档",Location=new Point(20,351),Size=new Size(300,30),Checked=false};dialog.Controls.Add(saves);
                    var image=new CheckBox{Text="选择一张截图作为附件",Location=new Point(345,351),Size=new Size(320,30),Checked=false};dialog.Controls.Add(image);
                    var ok=new Button{Text="导出选定会话",DialogResult=DialogResult.OK,Location=new Point(490,392),Size=new Size(180,34)};dialog.Controls.Add(ok);dialog.AcceptButton=ok;
                    if(dialog.ShowDialog(this)!=DialogResult.OK)return;var picked=new List<string>();foreach(int i in list.CheckedIndices)picked.Add(sessions[i]);selected=picked.ToArray();includeSaves=saves.Checked;includeImage=image.Checked;
                }
                string imageAttachment=null;
                if(includeImage)using(var image=new OpenFileDialog{Title="明确选择要附带的截图",Filter="图片 (*.png;*.jpg;*.jpeg)|*.png;*.jpg;*.jpeg"})
                {if(image.ShowDialog(this)!=DialogResult.OK)return;imageAttachment=image.FileName;}
                using(var dialog=new SaveFileDialog{Filter="诊断包 (*.zip)|*.zip",FileName="DeadSpace-Diagnostics-"+DateTime.Now.ToString("yyyyMMdd-HHmmss")+".zip",OverwritePrompt=false})
                {
                    if(dialog.ShowDialog(this)!=DialogResult.OK)return;diagnosticExporting=true;launch.Enabled=false;status.Text="正在后台打包选定诊断；原始记录保留。";
                    string dataRoot=SelectedDataRoot(),destination=dialog.FileName;
                    await Task.Run(()=>{
                        string saveAttachment=null;
                        try{
                            if(includeSaves){saveAttachment=destination+"."+Guid.NewGuid().ToString("N")+".selected-saves.zip";SaveBundle.Export(dataRoot,saveAttachment);}
                            DiagnosticFiles.Export(selected,destination,saveAttachment,imageAttachment);
                        }finally{if(saveAttachment!=null&&File.Exists(saveAttachment))File.Delete(saveAttachment);}
                    });status.Text="诊断ZIP已导出；包含所选会话和明确选择的附件，原始记录保留。";
                }
            }
            catch(Exception error){status.Text="未导出诊断："+error.Message+" 原始记录保留。";}
            finally{diagnosticExporting=false;RefreshGameState();}
        }
    }
}

