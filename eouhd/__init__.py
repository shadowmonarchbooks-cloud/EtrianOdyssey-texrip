__version__ = '0.12.0'

# 0.13 freezes the existing parser implementation while hardening destructive
# workspace boundaries through a compatibility overlay. Keeping the legacy
# module intact makes its behavior a stable reference for the Rust rewrite.
from .workspace_overhaul import install as _install_workspace_overhaul

_install_workspace_overhaul()
del _install_workspace_overhaul
