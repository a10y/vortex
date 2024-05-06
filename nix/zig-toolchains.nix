{ pkgs }:
  {
    findByVersion = version: with pkgs;
      lib.last (
        builtins.attrValues (lib.filterAttrs (n: v:
          v.version == version
        ) pkgs.zigpkgs));
  }
