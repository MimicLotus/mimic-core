# 🦖 MIMIC ARCHITECTURE & AGENT SPECIFICATION (AGY_SPEC.md)

## 1. System Vision & Purpose
Mimic is a unified, next-generation package management and system intelligence engine designed to replace `pacman`, `makepkg`, `paru`, and `devtools` on Arch Linux and Arch-based operating systems.

It provides a single high-performance Rust toolchain combining:
1. **Native `libalpm` Core**: Direct interaction with `/var/lib/pacman/` without invoking legacy C pacman.
2. **AUR & Micro-Repo Resolver**: Automatic resolution across GitHub Releases micro-repositories, Arch official CDN, and AUR `.SRCINFO`.
3. **Hermetic Source Builder**: Unprivileged `bwrap` container compilation injecting `-march=native -O3 -pipe -flto=auto`.
4. **Decoupled AI Mentor (`mimic-brain`)**: Dormant, zero-idle-RAM offline advisor over `/run/mimic/brain.sock` providing `mimic why <pkg>` insights and compilation failure triage.

### ⚠️ Mandatory System Base Dependencies
For all Mimic installation images, ISO profiles, and package manifests, the following toolchain components are **mandatory base dependencies** (`depends`):
* `bubblewrap` (`bwrap` unprivileged namespace isolation)
* `sccache` (shared C/C++/Rust compilation caching)
* `mold` (high-performance ELF linker)

---

## 2. Workspace Crate Architecture

### `crates/mimic-core` (Binary: `mimic`)
* **Role**: The unbreakable system package manager.
* **Size Target**: ~10 MB.
* **Strict Constraints**:
  * Absolutely NO heavy LLM dependencies, PyTorch/CUDA libraries, or non-deterministic compute kernels.
  * Must be robust enough to serve as the `base` package provider (`provides=('pacman')`).
  * Must support `--root <path>` and `--dbpath <path>` for 100% safe isolated testing.
* **Responsibilities**:
  * ALPM Sync (`-Sy`), Search (`-Ss`), Install (`-S`), Remove (`-R`), Query (`-Qi`).
  * Micro-Repo priority fetching (`https://github.com/user/repo/releases/download/.../mimic.db.tar.zst`).
  * AUR RPC querying and in-memory `.SRCINFO` parsing.
  * Bubblewrap (`bwrap`) containerized compilation.
  * Advisor IPC client (`/run/mimic/brain.sock` or `mimic-brain --diagnose`).

### `crates/mimic-brain` (Binary: `mimic-brain`)
* **Role**: Decoupled intelligent advisor & compiler triage daemon.
* **Size Target**: ~50 MB + quantized model weights.
* **Strict Constraints**:
  * Must be completely optional. Core package manager works 100% when `mimic-brain` is absent.
  * Must be **dormant / cold**: 0 MB RAM when idle; loads via mmap only when queried, generates response, and immediately frees memory.
* **Responsibilities**:
  * Listens on `/run/mimic/brain.sock` or executes standalone via CLI (`mimic-brain --why <pkg>`, `mimic-brain --diagnose`).
  * Local ArchWiki RAG vector store.
  * Intelligent triage of compiler OOMs, missing headers, and linker failures.

---

## 3. Communication Protocol (IPC)

When `mimic-core` encounters an error or `why` command:
1. Check if `/run/mimic/brain.sock` exists and is listening.
2. If yes, send JSON payload:
   ```json
   {
     "type": "diagnostic",
     "command": "mimic build hyprland-git",
     "exit_code": 1,
     "stderr_tail": "clang: error: unable to execute command: Killed\nninja: build stopped...",
     "package_name": "hyprland-git"
   }
   ```
3. If socket is absent, attempt `which mimic-brain` and spawn with `--diagnose <json>`.
4. If `mimic-brain` is not installed, output clean hint: `Tip: Install 'mimic-brain' for local AI diagnostics and package explanations.`

---

## 4. Testing & Safety Principles
* **Safe Sandbox First**: Never test unvetted ALPM transaction changes against host `/var/lib/pacman/`. Always use `--root /tmp/mimic-sandbox`.
* **Database Lock Integrity**: Ensure `alpm` transaction release hooks always execute cleanly, even on `SIGINT` / `Ctrl+C`.
