# 📋 MIMIC v4: TASK & MILESTONE TRACKER (task.md)

---

## 🏗️ Core Architecture Milestones

- [x] **Phase 1: Workspace & Spec Anchor**
  - Multi-crate Cargo workspace (`mimic-core`, `mimic-brain`).
  - Persistent architectural blueprint in `AGY_SPEC.md`.
  - Purged legacy hybrid artifacts while preserving codebase and assets.

- [x] **Phase 2: ALPM Core Engine & Safe Sandbox Mode**
  - `/etc/pacman.conf` mirror and repo parsing.
  - Native CPU feature tier probing (`x86_64-v3`, `v4`, `generic`) with auto CachyOS repo injection.
  - Transaction planning and commits with isolated `--root` and `--dbpath` sandbox switches.

- [x] **Phase 3: Native AUR RPC & GitHub Micro-Repo Resolver**
  - Async AUR v5 search and package metadata queries.
  - GitHub Releases API integration for precompiled `.pkg.tar.zst` micro-repo binary distribution.
  - Integrated fallback search displaying official, micro-repo, and AUR badges.

- [x] **Phase 4: Hermetic Sandbox Builder & Native Git Source Runner**
  - Unprivileged `bwrap` container execution with `--unshare-user`.
  - Integration with `sccache` compilation caching and `mold` modern linker.
  - Host CPU optimization injection (`-march=native -O3 -pipe -flto=auto`).
  - Snapshot fetch, build, and `.pkg.tar.zst` artifact generation.
  - **Builder Stability & Memory Safety Overhaul**:
    * Dynamic `MemoryGuard` budgeting: caps compilation jobs to safe concurrency `(RAM_MiB / 1536).clamp(1, nproc - 1)` (caps to 2–3 threads on ~5.6 GiB systems).
    * Explicit `-j {safe_jobs}` enforcement across Ninja, CMake, Make, and Cargo.
    * NVMe Workspace Migration: relocated all build, staging, and cache scratch dirs from tmpfs (`/tmp`) to persistent NVMe (`/var/tmp/mimic-build`).
    * Adaptive Optimization: throttles `-flto` and falls back to `-O2 -pipe` on systems with $\le 8\text{ GiB}$ RAM.
    * Background PSI Watcher: freezes sandbox process groups with `SIGSTOP` if memory pressure exceeds 25%, resuming with `SIGCONT` once pressure normalizes.

- [x] **Phase 5: Decoupled AI Mentor & Advisor IPC (`mimic-brain`)**
  - Dormant Unix domain socket IPC (`/run/mimic/brain.sock` / `~/.cache/mimic/brain.sock`).
  - Diagnostic triage heuristics (missing C/C++ headers, pkg-config, CMake packages, `-Werror`, undefined references).
  - Offline package explanations and ArchWiki insights (`mimic why <pkg>`).
  - 0 MB idle RAM overhead when dormant.

---

## 📦 Packaging & ISO Milestones

> [!IMPORTANT]
> **Mandatory Base Dependencies**: `bubblewrap`, `sccache`, and `mold` are **mandatory base dependencies** for the Mimic installation image and package manifest.
> Every standard Mimic system installation and live ISO must include these packages in the base system manifest to guarantee hermetic builds, hardware-accelerated compilation caching, and ultra-fast link times out of the box.

- [x] **Package Manifest Specification (`PKGBUILD`)**
  - Defines `bubblewrap`, `sccache`, `mold`, `libalpm.so=16`, `openssl`, `glibc`, and `gcc-libs` as **mandatory base dependencies** (`depends=(...)`).
  - Declares `provides=('aur-helper' 'makepkg')`.
  - Installs `mimic` and `mimic-brain` release binaries to `/usr/bin/`.
  - Installs systemd socket activation units (`mimic-brain.socket`, `mimic-brain.service`).

- [ ] **Live ISO Image Profile (`archiso` / MimicOS ISO)**
  - Base packages manifest inclusion:
    - `bubblewrap` (Hermetic container isolation)
    - `sccache` (Persistent build acceleration cache)
    - `mold` (High-speed default ELF linker)
    - `base-devel`, `git`, `rust`, `clang`, `gcc`
  - Enable `mimic-brain.socket` by default for zero-RAM on-demand triage.
  - Provide root configuration `/etc/mimic.conf` default profile.

- [x] **Automated GitHub Micro-Repo CI/CD**
  - GitHub Actions workflow for automated release building via `.github/workflows/release.yml`.
  - Auto-publishes generated `.pkg.tar.zst`, `mimic.db.tar.zst`, and standalone release binaries upon tag push.

---

## 🚀 Immediate Launch Sequence

- [ ] **Step 1: Release Tag & Micro-Repo Generation (`v4.0.0`)**
  - Create and push signed git release tag `v4.0.0` to `origin/main`.
  - Verify GitHub Actions workflow completes cleanly:
    * Compiles release binaries (`mimic`, `mimic-brain`).
    * Packages `.pkg.tar.zst` and generates `mimic.db.tar.zst` / `mimic.files.tar.zst`.
    * Creates GitHub Release with assets attached.

- [ ] **Step 2: AUR Package Submission (`mimic-bin` / `mimic-git`)**
  - Generate source manifest `.SRCINFO` with `makepkg --printsrcinfo > .SRCINFO`.
  - Push PKGBUILD to AUR repository for community installation via standard pacman / AUR helpers.

- [ ] **Step 3: MimicOS Live ISO Integration**
  - Update `archiso` packages list to include `bubblewrap`, `sccache`, `mold`, `git`, and `mimic`.
  - Enable `mimic-brain.socket` in systemd presets.
  - Test live ISO boot and verify zero-configuration sandbox package building.

---

## 🔮 Future Scope: mimic-brain v2

Because `mimic-brain` is isolated behind a domain socket (`/run/mimic/brain.sock`) with systemd socket activation, it can grow sophisticated diagnostic capabilities without ever compromising system responsiveness or base RAM usage.

- [ ] **Future: mimic-brain v2**
  - **Autonomous Build Triage & Auto-Patching**:
    * When a source build fails inside `bwrap`, `mimic-brain` inspects trailing stderr compiler logs.
    * Maps missing headers/libraries (e.g., `<wayland-server.h>`) directly to their exact Arch package provider (`wayland`).
    * Prompts interactively: *"Header missing. Install `wayland` to sandbox makedepends and retry? [Y/n]"*.
  - **Interactive Dependency Conflict Resolver**:
    * Detects file collisions, soname bumps, or circular dependency deadlocks before transactions commit.
    * Provides human-readable rollback or substitution strategies instead of cryptic libalpm error codes.
  - **Smart System Hygiene & Drift Analysis (`mimic audit`)**:
    * Detects unmanaged or orphaned configuration files, dangling build caches, and stale `.pacnew` merges.
    * Suggests safe pruning options and verifies package database integrity.
  - **ArchWiki Offline Embeddings**:
    * Bundles a lightweight, quantized vector index or targeted lookup table of common ArchWiki troubleshooting guides.
    * Enables high-fidelity offline assistance during network outages or emergency recovery boots.

