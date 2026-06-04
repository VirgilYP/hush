#!/bin/sh
set -eu

usage() {
    cat <<'EOF'
Usage: scripts/install-codex-skill.sh [options]

Installs the bundled hush-uart-console skill.

Options:
  --dest <skills-root>     Install under this skills root.
                           Default: $CODEX_HOME/skills or ~/.codex/skills.
  --target <skill-dir>     Install to this exact skill directory.
  --name <skill-name>      Skill directory name. Default: hush-uart-console.
  --force                  Replace an existing installed skill.
  --dry-run                Print actions without copying files.
  -h, --help               Show this help.

Examples:
  scripts/install-codex-skill.sh
  scripts/install-codex-skill.sh --dest ~/.codex/skills
  scripts/install-codex-skill.sh --target /tmp/customer-skills/hush-uart-console --force
EOF
}

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
source_dir="$repo_root/skills/hush-uart-console"
skill_name="hush-uart-console"
dest_root="${CODEX_HOME:-$HOME/.codex}/skills"
target_dir=""
force=0
dry_run=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        --dest)
            [ "$#" -ge 2 ] || {
                echo "error: --dest requires a path" >&2
                exit 2
            }
            dest_root=$2
            shift 2
            ;;
        --target)
            [ "$#" -ge 2 ] || {
                echo "error: --target requires a path" >&2
                exit 2
            }
            target_dir=$2
            shift 2
            ;;
        --name)
            [ "$#" -ge 2 ] || {
                echo "error: --name requires a skill name" >&2
                exit 2
            }
            skill_name=$2
            shift 2
            ;;
        --force)
            force=1
            shift
            ;;
        --dry-run)
            dry_run=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "error: unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if [ ! -f "$source_dir/SKILL.md" ]; then
    echo "error: missing bundled skill: $source_dir/SKILL.md" >&2
    exit 1
fi

if [ -z "$target_dir" ]; then
    target_dir="$dest_root/$skill_name"
fi

echo "source: $source_dir"
echo "target: $target_dir"

if [ "$dry_run" -eq 1 ]; then
    echo "dry-run: install skipped"
    exit 0
fi

if [ -e "$target_dir" ] && [ "$force" -ne 1 ]; then
    echo "error: target already exists; rerun with --force to replace it" >&2
    exit 1
fi

parent_dir=$(dirname -- "$target_dir")
mkdir -p "$parent_dir"

tmp_dir="${target_dir}.tmp.$$"
rm -rf "$tmp_dir"
mkdir -p "$tmp_dir"
cp -R "$source_dir/." "$tmp_dir/"

if [ -e "$target_dir" ]; then
    rm -rf "$target_dir"
fi
mv "$tmp_dir" "$target_dir"

echo "installed hush skill to: $target_dir"
