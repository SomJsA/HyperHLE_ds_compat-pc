# ASCII-only on purpose: PowerShell 5.1 reads a BOM-less script as GBK, so any
# non-ASCII literal here would be mangled. English prompts only.
#
# Asks for the synthetic shake parameters and writes run-host\ds-shake.txt.
# Invoked by the emulator when V is pressed (see window.rs).
Add-Type -AssemblyName Microsoft.VisualBasic
$title  = 'Dead Space - synthetic shake'
$prompt = 'amplitude(g), duration(s), frequency(Hz), axis   [axis = z/x/y/all/faithful]   tested jump: 6,3,4,z'
$s = [Microsoft.VisualBasic.Interaction]::InputBox($prompt, $title, '6,3,4,z')
if ([string]::IsNullOrWhiteSpace($s)) {
  'cancel' | Out-File -Encoding ASCII -NoNewline ds-shake.txt
} else {
  $s | Out-File -Encoding ASCII -NoNewline ds-shake.txt
}
