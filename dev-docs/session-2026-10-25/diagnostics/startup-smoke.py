from pathlib import Path
import shutil,subprocess,time,json,hashlib,os
base=Path(r'E:\CodexProjects\DeadSpaceLegacy')
stage=base/'builds/ending-head-circle-20261025'
runtime=base/'run-host/Runtime-PC'
smoke=stage/'startup-smoke'
smoke.mkdir(parents=True,exist_ok=True)
for name in ['touchHLE_dylibs','touchHLE_fonts']:
 shutil.copytree(runtime/name,smoke/name,dirs_exist_ok=True)
for name in ['touchHLE_default_options.txt','touchHLE_license.txt']:
 if (runtime/name).is_file():shutil.copy2(runtime/name,smoke/name)
# Run only this isolated copy. Never use/restore the foreground game's sandbox.
exe=stage/'touchHLE.exe'
if not exe.exists():raise SystemExit('Prepared isolated resource directory; candidate not staged yet.')
app=base/'device-app/extracted/Dead Space.app'
env=os.environ.copy();env.pop('TOUCHHLE_DS_CARRIE_HEAD_FIX',None)
log=smoke/'startup-output.txt'
with log.open('wb') as out:
 proc=subprocess.Popen([str(exe),str(app),'--headless'],cwd=smoke,env=env,stdout=out,stderr=subprocess.STDOUT,creationflags=subprocess.CREATE_NO_WINDOW)
 start=time.monotonic();matched=False
 try:
  while time.monotonic()-start<25:
   text=log.read_text(encoding='utf-8',errors='replace')
   matched='[DScarrie] verified 1.3.34' in text
   if matched or proc.poll() is not None:break
   time.sleep(.2)
 finally:
  if proc.poll() is None:proc.terminate()
  proc.wait(timeout=10)
result={'isolated_pid':proc.pid,'patch_applied':matched,'log':str(log),'foreground_untouched':True,'is_visual_acceptance':False}
(smoke/'result.json').write_text(json.dumps(result,indent=2),encoding='utf-8')
print(json.dumps(result))
if not matched:
 print(log.read_text(encoding='utf-8',errors='replace')[-2500:]);raise SystemExit(1)
