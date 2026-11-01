{
  description = "imap-statistics: statistics about the emails in IMAP accounts";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
  };

  outputs =
    { nixpkgs, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems =
        function: nixpkgs.lib.genAttrs systems (system: function nixpkgs.legacyPackages.${system});
    in
    {
      # The Rust toolchain, and pandoc with typst, so that the report and
      # examples/demo/generate.sh also write the PDF.
      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = [
            pkgs.pandoc
            pkgs.rustup
            pkgs.typst
          ];
        };
      });
      formatter = forAllSystems (pkgs: pkgs.nixfmt-tree);
    };
}
