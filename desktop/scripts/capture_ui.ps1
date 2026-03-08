$ErrorActionPreference = "Stop"

$preview = Start-Process -FilePath "cmd.exe" `
  -ArgumentList "/c", "node_modules\.bin\vite.cmd", "preview", "--host", "127.0.0.1", "--port", "4173" `
  -WorkingDirectory (Get-Location) `
  -PassThru

try {
  Start-Sleep -Seconds 2

  if (!(Test-Path "artifacts")) {
    New-Item -ItemType Directory -Path "artifacts" | Out-Null
  }

  & "node_modules\.bin\playwright.cmd" screenshot --device="Desktop Chrome" "http://127.0.0.1:4173" "artifacts/browser-preview.png"
  if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
  }
}
finally {
  if ($preview -and !$preview.HasExited) {
    Stop-Process -Id $preview.Id -Force
  }
}
