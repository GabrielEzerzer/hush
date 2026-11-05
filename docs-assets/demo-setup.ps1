# Prepares a throwaway vault and repo for demo.tape. Fake values only.
# The demo folder is $env:HUSH_DEMO_DIR, or a new folder under $env:TEMP.
$root = if ($env:HUSH_DEMO_DIR) { $env:HUSH_DEMO_DIR } else { Join-Path $env:TEMP ("hush-demo-" + (Get-Random)) }
if (-not (Test-Path $root)) { New-Item -ItemType Directory $root | Out-Null }
$env:HUSH_HOME = Join-Path $root "vault"
$env:HUSH_KEY = "42" * 32            # fixed fake key, so the real keychain is never used
Remove-Item Env:CLAUDECODE -ErrorAction SilentlyContinue
Remove-Item Env:NO_COLOR -ErrorAction SilentlyContinue
$env:CLICOLOR_FORCE = "1"         # keep colours when the recorder's terminal looks like a pipe
hush setup 2>&1 | Out-Null
[IO.File]::WriteAllText((Join-Path $root "stripe-key.txt"), "sk_test_not_a_real_key_0000")
$app = Join-Path $root "my-app"
New-Item -ItemType Directory -Force $app | Out-Null
Set-Location $app
git init -q
[IO.File]::WriteAllText((Join-Path $app "app.py"), @'
import os
key = os.environ.get("STRIPE_KEY")
print("app: STRIPE_KEY is set," if key else "app: STRIPE_KEY is missing", len(key or ""), "characters")
'@)
function global:prompt { "PS my-app> " }
