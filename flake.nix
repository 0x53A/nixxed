{
  description = "NixOS TUI configuration editor";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        
        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
        
        nixxed = pkgs.rustPlatform.buildRustPackage {
          pname = cargoToml.package.name;
          version = cargoToml.package.version;
          
          src = ./.;
          
          cargoLock = {
            lockFile = ./Cargo.lock;
            allowBuiltinFetchGit = true;
          };
          
          nativeBuildInputs = [ pkgs.pkg-config ];
          
          meta = with pkgs.lib; {
            description = "NixOS TUI configuration editor";
            homepage = "https://github.com/0x53A/nixxed";
            license = licenses.mit;
            maintainers = [];
            mainProgram = "nixxed";
          };
        };
      in
      {
        packages = {
          default = nixxed;
          nixxed = nixxed;
        };
        
        apps.default = {
          type = "app";
          program = "${nixxed}/bin/nixxed";
        };
        
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rustup
            rust-analyzer
            pkg-config
          ];
        };
      }
    ) // {
      # NixOS module for easy integration
      nixosModules.default = { config, pkgs, lib, ... }: {
        options.programs.nixxed = {
          enable = lib.mkEnableOption "nixxed, a NixOS TUI configuration editor";
        };
        
        config = lib.mkIf config.programs.nixxed.enable {
          environment.systemPackages = [ self.packages.${pkgs.system}.default ];
        };
      };
    };
}
