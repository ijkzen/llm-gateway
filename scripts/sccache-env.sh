#!/bin/sh

set -eu

project_name="${SCCACHE_PROJECT:-llm-gateway}"
os_name="${SCCACHE_OS:-$(uname -s)}"
arch_name="${SCCACHE_ARCH:-$(uname -m)}"

case "${os_name}" in
  Linux | linux)
    os_name="linux"
    ;;
  Darwin | darwin)
    os_name="darwin"
    ;;
  *)
    os_name=$(printf '%s' "${os_name}" | tr '[:upper:]' '[:lower:]')
    ;;
esac

case "${arch_name}" in
  x86_64 | amd64)
    arch_name="amd64"
    ;;
  aarch64 | arm64)
    arch_name="arm64"
    ;;
  *)
    arch_name=$(printf '%s' "${arch_name}" | tr '[:upper:]' '[:lower:]')
    ;;
esac

default_prefix="${project_name}/${os_name}/${arch_name}"
backend="${SCCACHE_BACKEND:-s3://rust_build_cache/${default_prefix}}"
backend_without_scheme="${backend#s3://}"

if [ "${backend_without_scheme}" = "${backend}" ] || [ -z "${backend_without_scheme}" ]; then
  echo "warning: unsupported SCCACHE_BACKEND '${backend}', expected s3://<bucket>/<prefix>; using defaults" >&2
  sccache_bucket="rust_build_cache"
  sccache_prefix="${default_prefix}"
else
  sccache_bucket="${backend_without_scheme%%/*}"
  sccache_prefix="${backend_without_scheme#*/}"

  if [ -z "${sccache_bucket}" ] || [ -z "${sccache_prefix}" ] || [ "${sccache_prefix}" = "${backend_without_scheme}" ]; then
    echo "warning: invalid SCCACHE_BACKEND '${backend}', expected s3://<bucket>/<prefix>; using defaults" >&2
    sccache_bucket="rust_build_cache"
    sccache_prefix="${default_prefix}"
  fi
fi

export SCCACHE_PROJECT="${project_name}"
export SCCACHE_OS="${os_name}"
export SCCACHE_ARCH="${arch_name}"
export SCCACHE_BACKEND="${backend}"
export SCCACHE_BUCKET="${SCCACHE_BUCKET:-${sccache_bucket}}"
export SCCACHE_S3_KEY_PREFIX="${SCCACHE_S3_KEY_PREFIX:-${sccache_prefix}}"
export SCCACHE_S3_ENDPOINT="${SCCACHE_S3_ENDPOINT:-http://192.168.31.100:2041}"
export SCCACHE_ENDPOINT="${SCCACHE_ENDPOINT:-${SCCACHE_S3_ENDPOINT}}"
export SCCACHE_S3_USE_SSL="${SCCACHE_S3_USE_SSL:-false}"
export SCCACHE_REGION="${SCCACHE_REGION:-us-east-1}"
export AWS_ACCESS_KEY_ID="${AWS_ACCESS_KEY_ID:-admin}"
export AWS_SECRET_ACCESS_KEY="${AWS_SECRET_ACCESS_KEY:-password}"
