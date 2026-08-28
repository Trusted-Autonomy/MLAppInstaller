# Windows equivalent of setup.sh -- see that file for the full rationale.
$Project = "none"
for ($i = 0; $i -lt $args.Length; $i++) {
    if ($args[$i] -eq "-Project") {
        $Project = $args[$i + 1]
    }
}

"bound project: $Project" | Out-File -FilePath "bound-project.txt"
New-Item -ItemType File -Path "marker.txt" -Force | Out-Null
