# Records docs-assets/demo.gif with charmbracelet/vhs. Run from the repo root
# with hush on PATH:  vhs docs-assets/demo.tape
Output docs-assets/demo.gif

Set Shell "powershell"
Set FontSize 16
Set Width 1180
Set Height 450
Set Padding 24
Set Theme "Catppuccin Mocha"
Set TypingSpeed 35ms

Hide
Type ". ./docs-assets/demo-setup.ps1"
Enter
Sleep 2s
Type "Clear-Host"
Enter
Sleep 500ms
Show

Type "hush init"
Enter
Sleep 2.5s

Type "Get-Content ..\stripe-key.txt | hush set STRIPE_KEY"
Enter
Sleep 1.5s

Type "hush list"
Enter
Sleep 1.5s

Type "hush run -- python app.py"
Enter
Sleep 2s

Type "# inside an AI agent's shell, CLAUDECODE is set:"
Enter
Type "$env:CLAUDECODE = 1; hush get STRIPE_KEY"
Enter
Sleep 4s
