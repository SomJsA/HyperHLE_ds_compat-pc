param(
    [Parameter(Mandatory=$true)][string]$OutputDirectory,
    [Parameter(Mandatory=$true)][string]$SeedIni
)
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Path $root -Force | Out-Null
$case = Join-Path $root ('fire-settings-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $case | Out-Null
Copy-Item -LiteralPath $SeedIni -Destination (Join-Path $case 'DeadSpace-Launcher.ini')
# Test forms are never shown; only this new private directory is written.
$source = @'
using System;using System.Diagnostics;using System.IO;using System.Windows.Forms;
namespace DeadSpaceBootstrap {
 internal static class FireModeSettingsTestMain {
  [STAThread] static int Main(){try{Application.EnableVisualStyles();LauncherForm.TestFireModeSettings();Console.WriteLine("PASS: eight independent fire/caption settings save/reload combinations; forms never shown");return 0;}catch(Exception e){Console.WriteLine(e);return 1;}}
 }
 internal sealed partial class LauncherForm {
  internal static void TestFireModeSettings(){
   using(var defaults=new LauncherForm()){if(defaults.Visible)throw new Exception("Test form visible");if(defaults.padFireMode.SelectedIndex!=1||defaults.keyboardAltMode.SelectedIndex!=0)throw new Exception("Unexpected defaults");}
   for(int p=0;p<2;p++)for(int k=0;k<2;k++)for(int caption=0;caption<2;caption++){
    using(var form=new LauncherForm()){
     if(Path.GetFullPath(form.baseDir).TrimEnd(Path.DirectorySeparatorChar)!=Path.GetFullPath(AppDomain.CurrentDomain.BaseDirectory).TrimEnd(Path.DirectorySeparatorChar))throw new Exception("Private base path mismatch");
     form.padFireMode.SelectedIndex=p;form.keyboardAltMode.SelectedIndex=k;form.subtitleMode.SelectedIndex=caption;form.SaveSettings();
     var info=new ProcessStartInfo();form.ApplyFireModeEnvironment(info);
     if(info.EnvironmentVariables["TOUCHHLE_DS_CONTROLLER_FIRE_MODE"]!=(p==0?"split":"toggle")||info.EnvironmentVariables["TOUCHHLE_DS_KEYBOARD_ALT_MODE"]!=(k==0?"hold":"toggle"))throw new Exception("Environment mismatch");
    }
    using(var reload=new LauncherForm()){if(reload.padFireMode.SelectedIndex!=p||reload.keyboardAltMode.SelectedIndex!=k||reload.subtitleMode.SelectedIndex!=caption)throw new Exception("Independent roundtrip mismatch");if(reload.Visible)throw new Exception("Reload form visible");}
   }
  }
 }
}

'@
$testSource = Join-Path $case 'FireModeSettingsTests.cs'
[IO.File]::WriteAllText($testSource, $source, [Text.UTF8Encoding]::new($false))
$compiler = Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
$exe = Join-Path $case 'FireModeSettingsTests.exe'
$poster = Join-Path $PSScriptRoot 'assets/DeadSpace-Poster.png'
$profile = Join-Path $PSScriptRoot 'DeadSpace-VisualCompat.ini'
$sources = @('Program.cs','LauncherShell.cs','TextureSettings.cs','HardwareDefaults.cs',
    'SamplingSettings.cs','SaveSettings.cs','KeyBindings.cs','SaveStorage.cs',
    'StorageControls.cs','GamepadBindings.cs','SaveDocument.cs','SaveEditor.cs',
    'SaveBackup.cs','GameSession.cs','QuitConfirmation.cs','VisualCompatibility.cs') |
    ForEach-Object { Join-Path $PSScriptRoot $_ }
& $compiler /nologo /target:exe /platform:x64 /optimize+ /main:DeadSpaceBootstrap.FireModeSettingsTestMain "/out:$exe" "/resource:$poster,DeadSpaceBootstrap.DeadSpace-Poster.png" "/resource:$profile,DeadSpaceBootstrap.VisualCompat.ini" /r:System.Windows.Forms.dll /r:System.Drawing.dll $sources $testSource
if ($LASTEXITCODE -ne 0) { throw 'Fire-mode settings test compilation failed.' }
& $exe
if ($LASTEXITCODE -ne 0) { throw 'Fire-mode settings persistence/environment test failed.' }
[pscustomobject]@{passed=$true;combinations=8;formsShown=$false;gameStarted=$false;privateOutput=$case} |
    ConvertTo-Json | Set-Content -LiteralPath (Join-Path $case 'result.json') -Encoding UTF8
