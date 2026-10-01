# GitHub Credential Setup — ALL-BRIGHT

**Repository:** `ctemam/ALL-BRIGHT`
**Local path:** `D:\Allbright`
**Configured:** October 1, 2026

---

## Configuration Summary

| Item | Value |
|---|---|
| Git version | 2.52.0.windows.1 |
| Credential helper | Git Credential Manager (GCM) 2.6.1 |
| `credential.helper` | `manager` (global) |
| Remote URL | `https://github.com/ctemam/ALL-BRIGHT.git` |
| Auth method | Personal Access Token (PAT), cached by GCM |
| Credential store | Windows Credential Manager |
| Git identity | `clightafrica <clightafrica@gmail.com>` |

> The actual PAT is **not** stored in this file. It lives in Windows
> Credential Manager under `git:https://github.com`.

---

## Verify Setup

```bash
git --version                        # >= 2.27.0
git credential-manager --version     # should print a version
git config --global credential.helper  # should print: manager
git ls-remote origin                 # should work with no password prompt
```

## Push Workflow

```bash
cd /d/Allbright
git add .
git commit -m "Your message"
git push origin main
```

---

## Token Rotation

When the PAT expires (90-day expiry recommended):

1. Create a new token at <https://github.com/settings/tokens>
   - Scopes: `repo`, `workflow`
2. Clear the cached credential:

   ```bash
   printf "protocol=https\nhost=github.com\n\n" | git credential reject
   ```

3. Store the new token:

   ```bash
   printf "protocol=https\nhost=github.com\nusername=ctemam\npassword=<NEW_TOKEN>\n\n" | git credential approve
   ```

   Or just run `git push` and paste the new token when prompted.

---

## SSH (Alternative, currently unused)

An SSH remote alias `github.com-zca` exists in `~/.ssh/config` using key
`~/.ssh/allbright_deploy`, but that deploy key is **not authorized** on
the repo. To re-enable SSH:

1. Add `~/.ssh/zca_allbright_deploy.pub` (or a new key) as a deploy key:
   Repo → Settings → Deploy keys → Add deploy key (check "Allow write access")
2. Switch the remote back:

   ```bash
   git remote set-url origin git@github.com-zca:ctemam/ALL-BRIGHT.git
   ```

---

## Troubleshooting

| Error | Fix |
|---|---|
| `Authentication failed` | `git credential reject` (input `https` / `github.com`), then retry |
| PAT expired | Follow Token Rotation above |
| `command not found: git-credential-manager` | Reinstall from <https://github.com/git-ecosystem/git-credential-manager/releases> |
| Password prompt in VS Code | Set `"git.terminalAuthentication": false` in VS Code settings |
