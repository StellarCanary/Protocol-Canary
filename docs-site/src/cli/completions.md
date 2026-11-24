# `completions`

Prints a shell completion script on standard output, so you can save it to a
file or source it directly from your shell configuration.

```text
Print a shell completion script

Usage: stellar-canary completions <SHELL>

Arguments:
  <SHELL>
          Shell to generate a completion script for: bash, zsh, fish or powershell

          Possible values:
          - bash:       Bourne-Again shell (`bash`)
          - zsh:        Z shell (`zsh`)
          - fish:       Friendly interactive shell (`fish`)
          - powershell: Microsoft PowerShell (`powershell`)

Options:
  -h, --help
          Print help (see a summary with '-h')
```

## Supported shells

Exactly four shells are accepted: `bash`, `zsh`, `fish` and `powershell`.

```bash
# bash: once, for all users
stellar-canary completions bash > /etc/bash_completion.d/stellar-canary

# zsh: into any directory on your $fpath
stellar-canary completions zsh > "${fpath[1]}/_stellar-canary"

# fish
stellar-canary completions fish > ~/.config/fish/completions/stellar-canary.fish

# PowerShell
stellar-canary completions powershell > stellar-canary.ps1
```

Reload the shell (or open a new one) after installing the file.

## stdout is the script, stderr is everything else

The generated script is the only thing written to standard output, and a
successful run says nothing on standard error. That contract makes redirection
safe: `stellar-canary completions bash > file` writes exactly the script,
nothing more.

Errors and diagnostics go to standard error and never share stdout with a
script. An unsupported shell name — or a misspelling — is rejected by the
argument parser before anything is generated: the command exits with
[exit code `2`](../exit-codes.md), the error on stderr lists the supported
shells (`bash`, `zsh`, `fish`, `powershell`), and stdout stays empty. The
same applies to a missing shell argument. There is no default shell: input is
never silently mapped onto one.

## Generated from the real CLI

The script is produced from the same command definition the binary parses
with, so it always describes the actual command tree of your installed
version — every subcommand, option and flag, including ones added in later
releases. Nothing is duplicated by hand, so the completions cannot drift from
`stellar-canary --help`.
