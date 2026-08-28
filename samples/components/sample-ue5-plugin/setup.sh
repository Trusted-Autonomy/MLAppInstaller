#!/bin/sh
# Sample component demonstrating project binding (binds_to_project_type).
# On plain install, no project is bound yet -- just install and go healthy.
# When `mlai bind-project`/the GUI's "Bind a Project" panel runs, this
# script is re-invoked with the real path substituted for {project}.
set -eu

PROJECT="none"
while [ $# -gt 0 ]; do
  case "$1" in
    -Project)
      shift
      PROJECT="$1"
      ;;
  esac
  shift
done

echo "bound project: $PROJECT" > bound-project.txt
touch marker.txt
