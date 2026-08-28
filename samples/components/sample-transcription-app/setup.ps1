# Windows equivalent of setup.sh -- see that file for the full rationale.
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$Catalog = Join-Path $ScriptDir "catalog\trusted-autonomy.toml"

$SampleOs = if ($env:MLAI_SAMPLE_OS) { $env:MLAI_SAMPLE_OS } else { "windows" }
$SampleVendor = if ($env:MLAI_SAMPLE_GPU_VENDOR) { $env:MLAI_SAMPLE_GPU_VENDOR } else { "none" }
$SampleVram = if ($env:MLAI_SAMPLE_VRAM_GB) { $env:MLAI_SAMPLE_VRAM_GB } else { "0" }

if ($args[0] -eq "--describe-options") {
    $Resolved = & mlai catalog resolve `
        --purpose voice-transcription `
        --catalog $Catalog `
        --os $SampleOs `
        --gpu-vendor $SampleVendor `
        --vram-gb $SampleVram `
        --effective-vram-gb $SampleVram `
        --disk-free-gb 50
    if ($LASTEXITCODE -ne 0) { exit 1 }
    $Descriptor = @{
        schema_version = 1
        options = @(
            @{
                key = "model"
                label = "Transcription model (resolved for this machine)"
                type = "choice"
                choices = @(@{ value = $Resolved; label = $Resolved; recommended = $true })
                default = $Resolved
            }
        )
    } | ConvertTo-Json -Depth 5 -Compress
    Write-Output $Descriptor
    exit 0
}

$Model = "unset"
for ($i = 0; $i -lt $args.Length; $i++) {
    if ($args[$i] -eq "--set" -and $args[$i + 1] -like "model=*") {
        $Model = $args[$i + 1].Substring(6)
    }
}

"selected model: $Model" | Out-File -FilePath "selected-model.txt"
New-Item -ItemType File -Path "marker.txt" -Force | Out-Null
