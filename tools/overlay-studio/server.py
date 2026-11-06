"""Loopback-only screenshot/style bridge. Never drives gameplay or edits saves."""
from http.server import ThreadingHTTPServer, SimpleHTTPRequestHandler
from pathlib import Path
import json, math, os, subprocess, sys, time

ROOT = Path(__file__).parent.resolve()
ADB = Path(r"E:\CodexProjects\DeadSpaceLegacy\work\android-sdk-fsr3\platform-tools\adb.exe")
DEVICE = "10.31.0.232:42417"
PACKAGE = "org.touchhle.android.chaptertest"
RANGES = {"x":(0,100),"y":(0,100),"width":(10,96),"fontSize":(3,36),"secondaryFontSize":(3,30),"lineHeight":(1,2),"padding":(0,20),"radius":(0,20),"opacity":(0,1),"strokeOpacity":(0,1),"gap":(0,15),"maxHeight":(10,80)}

def validate(config):
    import re
    if not isinstance(config,dict) or config.get("schema")!=1 or not isinstance(config.get("layers"),dict):
        raise ValueError("需要 schema=1 的图文配置")
    defaults=json.loads((ROOT/"default-style.json").read_text(encoding="utf-8"))
    for name, base in defaults["layers"].items():
        config["layers"][name]={**base,**config["layers"].get(name,{})}
    def style(value,single=False):
        if not isinstance(value,dict): raise ValueError("样式必须为对象")
        for k,v in value.items():
            if single and k=="enabled":raise ValueError("单条覆盖不修改总开关")
            if k in RANGES:
                low,high=RANGES[k]
                if isinstance(v,bool) or not isinstance(v,(int,float)) or not math.isfinite(v) or not low<=v<=high:raise ValueError("参数超出范围："+k)
            if k in ["color","secondaryColor","background","stroke"] and (not isinstance(v,str) or not re.fullmatch(r"#[\da-fA-F]{6}",v)):raise ValueError("颜色无效："+k)
    for name,value in config["layers"].items():
        if name not in defaults["layers"]: raise ValueError("未知图层")
        style(value)
    overrides=config.setdefault("overrides",{})
    if not isinstance(overrides,dict) or len(overrides)>2000: raise ValueError("单条覆盖格式或数量无效")
    known={e["id"] for e in json.loads((ROOT/"samples.json").read_text(encoding="utf-8"))}
    for key,entry in overrides.items():
        if key not in known or not isinstance(entry,dict):raise ValueError("不存在的文本 ID："+str(key))
        for field in ["text","secondaryText"]:
            if field in entry and (not isinstance(entry[field],str) or not entry[field].strip() or len(entry[field])>4000):raise ValueError("条目文案无效："+key)
        if "style" in entry:style(entry["style"],True)
    return config

def adb(*args,timeout=25):
    p=subprocess.run([str(ADB),"-s",DEVICE,*args],capture_output=True,timeout=timeout)
    if p.returncode:raise RuntimeError(p.stderr.decode(errors="replace").strip() or p.stdout.decode(errors="replace").strip())
    return p.stdout

class Handler(SimpleHTTPRequestHandler):
    extensions_map = {**SimpleHTTPRequestHandler.extensions_map, ".mjs":"application/javascript", ".js":"application/javascript", ".otf":"font/otf"}
    def __init__(self,*args,**kwargs): super().__init__(*args,directory=str(ROOT),**kwargs)
    def end_headers(self):
        self.send_header("Cache-Control","no-store")
        self.send_header("X-Content-Type-Options","nosniff")
        super().end_headers()
    def result(self,value,status=200):
        data=json.dumps(value,ensure_ascii=False).encode("utf-8")
        self.send_response(status);self.send_header("Content-Type","application/json; charset=utf-8");self.send_header("Content-Length",str(len(data)));self.end_headers();self.wfile.write(data)
    def do_GET(self):
        if self.path=="/api/status":
            try:connected=adb("get-state").strip()==b"device"
            except Exception:connected=False
            return self.result({"connected":connected,"package":PACKAGE})
        return super().do_GET()
    def do_POST(self):
        if self.headers.get("Origin") not in [None,"http://127.0.0.1:8768","http://localhost:8768"]:
            return self.result({"error":"仅允许本地预览器调用"},403)
        if not self.headers.get("Content-Type","").startswith("application/json"):return self.result({"error":"需要 JSON"},415)
        try:
            length=int(self.headers.get("Content-Length","0"))
            if length<0 or length>1048576:raise ValueError("配置超过 1MB")
            body=json.loads(self.rfile.read(length) or b"{}")
            if self.path=="/api/capture":
                image=adb("exec-out","screencap","-p");name="tablet-current.png";(ROOT/"assets"/name).write_bytes(image)
                return self.result({"url":"assets/"+name+"?t="+str(time.time_ns())})
            if self.path!="/api/apply":return self.result({"error":"不存在的接口"},404)
            config=validate(body["config"]);target=body.get("target","android")
            if target not in ["pc","android","both"]:raise ValueError("应用目标无效")
            data=json.dumps(config,ensure_ascii=False,indent=2).encode("utf-8")
            pending=ROOT/"current-style.pending.json";pending.write_bytes(data);os.replace(pending,ROOT/"current-style.json")
            if target in ["pc","both"]:
                pending=ROOT/"pc-style.pending.json";pending.write_bytes(data);os.replace(pending,ROOT/"pc-style.json")
            if target in ["android","both"]:
                adb("push",str(ROOT/"current-style.json"),"/data/local/tmp/ds-overlay-style.json")
                # Run-as cannot traverse FUSE app storage on this tablet. Private
                # files/ is read directly by the game process, with app ownership.
                adb("shell","run-as",PACKAGE,"mkdir","-p","files")
                adb("shell","cat /data/local/tmp/ds-overlay-style.json | run-as "+PACKAGE+" tee files/overlay-style.pending.json >/dev/null")
                adb("shell","run-as",PACKAGE,"mv","files/overlay-style.pending.json","files/overlay-style.json")
            names={"android":"平板","pc":"PC","both":"PC 与平板"}
            return self.result({"message":"已写入"+names[target]+"配置，组件运行时约 1 秒内读取；总开关关闭时继续原生显示。","overrides":len(config["overrides"])})
        except Exception as error:return self.result({"error":str(error)},400)

if __name__=="__main__":
    print("Overlay Studio: http://127.0.0.1:8768",flush=True)
    ThreadingHTTPServer(("127.0.0.1",8768),Handler).serve_forever()
