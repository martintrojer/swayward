#!/usr/bin/env bash
# Create the pinned Arch environment used by contrib/probe-dms-sway.
set -euo pipefail

container=swayward-dms
image=docker.io/library/archlinux:latest
packages=(
    quickshell=0.3.1-1
    dms-shell=1.6.2-1
    dms-shell-hyprland=1.6.2-1
    sway=1:1.12-4
    foot=1.28.0-2
    jq=1.8.2-1
)

if ! distrobox list | awk -F ' *\\| *' -v name="$container" 'NR > 1 && $2 == name { found=1 } END { exit !found }'; then
    distrobox create --yes --name "$container" --image "$image"
fi

distrobox enter "$container" -- sudo pacman -Syu --needed --noconfirm "${packages[@]}"
distrobox enter "$container" -- pacman -Q "${packages[@]%%=*}"
