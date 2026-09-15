param(
    [string[]]$IncludePaths = @("nextgen", "rust_app", "asm", "src-rs"),
    [string[]]$IncludePatterns = @("*.rs", "*.asm", "*.s", "*.S", "*.inc", "*.h", "*.hpp", "*.c", "*.cpp", "*.toml"),
    [string[]]$ExcludeContains = @("/target/", "/build/", "/.git/"),
    [switch]$FailIfNoFiles
)

$ErrorActionPreference = "Stop"

function Test-FileNameMatches {
    param(
        [string]$FileName,
        [string[]]$Patterns
    )

    foreach ($pattern in $Patterns) {
        if ($FileName -like $pattern) {
            return $true
        }
    }

    return $false
}

function Test-IsExcluded {
    param(
        [string]$Path,
        [string[]]$Needles
    )

    $normalized = $Path.Replace('\\', '/')
    foreach ($needle in $Needles) {
        if ($normalized -like "*$needle*") {
            return $true
        }
    }

    return $false
}

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
Push-Location $repoRoot
try {
    $scanRoots = @()
    foreach ($relPath in $IncludePaths) {
        if (Test-Path $relPath) {
            $scanRoots += (Resolve-Path $relPath).Path
        }
    }

    if ($scanRoots.Count -eq 0) {
        Write-Host "[NO-SHORTCUTS] Aucun dossier cible present."
        if ($FailIfNoFiles.IsPresent) {
            Write-Host "[NO-SHORTCUTS] Echec car -FailIfNoFiles est actif."
            exit 2
        }

        exit 0
    }

    $candidateFiles = New-Object System.Collections.Generic.List[string]
    foreach ($root in $scanRoots) {
        Get-ChildItem -Path $root -Recurse -File |
            Where-Object {
                (Test-FileNameMatches -FileName $_.Name -Patterns $IncludePatterns) -and
                -not (Test-IsExcluded -Path $_.FullName -Needles $ExcludeContains)
            } |
            ForEach-Object {
                $candidateFiles.Add($_.FullName)
            }
    }

    if ($candidateFiles.Count -eq 0) {
        Write-Host "[NO-SHORTCUTS] Aucun fichier source correspondant aux patterns."
        if ($FailIfNoFiles.IsPresent) {
            Write-Host "[NO-SHORTCUTS] Echec car -FailIfNoFiles est actif."
            exit 3
        }

        exit 0
    }

    $rules = @(
        @{ Name = "mock"; Pattern = "(?i)\\bmock(s|ing|ed)?\\b" },
        @{ Name = "stub"; Pattern = "(?i)\\bstub(s|bing|bed)?\\b" },
        @{ Name = "todo_fixme"; Pattern = "(?i)\\b(TODO|FIXME|TBD|XXX)\\b" },
        @{ Name = "hardcode"; Pattern = "(?i)\\bhardcod(ed|e|ing)?\\b" },
        @{ Name = "dead_code"; Pattern = "(?i)\\bdead\\s*code\\b" },
        @{ Name = "placeholder"; Pattern = "(?i)\\b(not implemented|unimplemented)\\b" }
    )

    $allowMarker = "NOSHORTCUTS-ALLOW"
    $findings = New-Object System.Collections.Generic.List[object]

    foreach ($rule in $rules) {
        foreach ($file in $candidateFiles) {
            $matches = Select-String -Path $file -Pattern $rule.Pattern
            foreach ($m in $matches) {
                if ($m.Line -match $allowMarker) {
                    continue
                }

                $relative = [System.IO.Path]::GetRelativePath($repoRoot.Path, $file).Replace('\\', '/')
                $findings.Add([pscustomobject]@{
                    Rule = $rule.Name
                    File = $relative
                    Line = $m.LineNumber
                    Text = $m.Line.Trim()
                })
            }
        }
    }

    if ($findings.Count -gt 0) {
        Write-Host "[NO-SHORTCUTS] Violations detectees: $($findings.Count)"
        $findings |
            Sort-Object Rule, File, Line |
            ForEach-Object {
                Write-Host (" - [{0}] {1}:{2} => {3}" -f $_.Rule, $_.File, $_.Line, $_.Text)
            }
        exit 1
    }

    Write-Host "[NO-SHORTCUTS] OK: aucune violation detectee."
    exit 0
}
finally {
    Pop-Location
}
