# The library path an iced window is started with, so wgpu reaches the
# machine's GPU driver: sourced by every desktop launcher (flake.nix,
# `icedApp`) and by the dev shells, at the moment they start, because what
# it finds is the host's and moves when the host does.
#
# In: ARK_FLAKE_LIBS, the flake's own wayland, libxkbcommon, vulkan-loader,
# libGL (libglvnd) and X libraries as a library path; ARK_LDD, an ldd.
# Out: LD_LIBRARY_PATH (and, off NixOS, VK_ADD_DRIVER_FILES), exported.
#
# Why the order matters — the reasoning is the old harken repository's
# (k2on/harken's dev shell, on k2on/petros), where the GPU worked on the
# same Asahi machine this was found on:
#
#   NixOS keeps the host's GPU drivers under /run/opengl-driver, and they
#   are linked against the host's libwayland. LD_LIBRARY_PATH beats a
#   library's own RUNPATH, so on a machine tracking a newer channel than
#   this flake's pin, the flake's wayland shadows the one Mesa was built
#   for. Every Mesa Vulkan driver then fails to load ("undefined symbol:
#   wl_fixes_interface"), wgpu finds no adapter at all, and iced quietly
#   falls back to its software renderer.
#
# So, on NixOS (/run/opengl-driver/lib exists), first: the directory of
# every library the host's driver itself resolves, with our path out of
# the way, that is one we would otherwise put first — wayland, xkbcommon,
# the X libraries — so the host's own copy wins. Asked of the driver each
# launch rather than written down, so this keeps working when the host
# moves on again. Then /run/opengl-driver/lib (and -32), the driver, for
# the manifests that name a library rather than a path (NVIDIA's). Then
# the flake's own libraries, which are what the window uses for anything
# the host did not answer: the loaders (libvulkan, libEGL), which the host
# does not put in /run/opengl-driver and which look in
# /run/opengl-driver/share for the drivers by themselves, and everything
# off NixOS. Then whatever the caller had.
#
# Off NixOS nothing is the host's: the flake's loaders search the host's
# manifests (/usr/share/vulkan/icd.d, /etc, glvnd's egl_vendor.d), but the
# drivers those name are built against the host's C library and its own
# dependencies, which a program from nixpkgs generally cannot load. That is
# nixGL's job (README.md, "The desktop window and the GPU").
ark_gpu_env() {
  local front="" driver dir lib manifest seen=":"
  local -a drivers=()
  if [ -d /run/opengl-driver/lib ]; then
    drivers+=(/run/opengl-driver/lib/libvulkan_*.so /run/opengl-driver/lib/libEGL_mesa.so.0)
  fi
  # The drivers a caller named itself — nixGL's, off NixOS — are asked the
  # same, for nixGL's nixpkgs need not be this flake's either.
  local IFS=:
  for manifest in ${VK_ICD_FILENAMES:-} ${VK_DRIVER_FILES:-} ${__EGL_VENDOR_LIBRARY_FILENAMES:-}; do
    [ -f "$manifest" ] || continue
    lib=$(sed -n 's|.*"library_path"[[:space:]]*:[[:space:]]*"\([^"]*\)".*|\1|p' "$manifest" | head -1)
    case "$lib" in /*) drivers+=("$lib") ;; esac
  done
  unset IFS
  for driver in "${drivers[@]}"; do
    [ -e "$driver" ] || continue
    while read -r lib dir; do
      case "$lib" in
        libwayland-*|libxkbcommon*|libX11*|libXcursor*|libXi.*|libXrandr*|libXext*|libXrender*|libxcb*) ;;
        *) continue ;;
      esac
      case "$seen" in *":$dir:"*) continue ;; esac
      seen="$seen$dir:"
      front="${front:+$front:}$dir"
    done < <(LD_LIBRARY_PATH="" "$ARK_LDD" "$driver" 2>/dev/null \
      | sed -n 's|^[[:space:]]*\([^[:space:]]*\) => \(/[^[:space:]]*\)/[^/[:space:]]* (.*|\1 \2|p')
  done
  if [ -d /run/opengl-driver/lib ]; then
    front="${front:+$front:}/run/opengl-driver/lib"
    if [ -d /run/opengl-driver-32/lib ]; then
      front="$front:/run/opengl-driver-32/lib"
    fi
  elif [ -z "${VK_ICD_FILENAMES:-}${VK_DRIVER_FILES:-}" ]; then
    # The host's Vulkan manifests, where a distribution keeps them, for a
    # loader whose XDG_DATA_DIRS fallback is nixpkgs' empty one. Added to,
    # never replacing, what the loader finds; nixGL's own wins when set.
    local add="${VK_ADD_DRIVER_FILES:-}"
    for dir in /usr/share/vulkan/icd.d /etc/vulkan/icd.d; do
      [ -d "$dir" ] && add="${add:+$add:}$dir"
    done
    if [ -n "$add" ]; then
      export VK_ADD_DRIVER_FILES="$add"
    fi
  fi
  export LD_LIBRARY_PATH="${front:+$front:}$ARK_FLAKE_LIBS${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
}
ark_gpu_env
unset -f ark_gpu_env
