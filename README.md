# Description

\<todo>

# Installation

## Flake

If you have enabled flakes in your system (``nix.settings.experimental-features = [ "nix-command" "flakes" ];``), you can run this via ``nix run github:0x53A/nixxed``.

## As a custom module

I have installed it system-wide by added a new file next to ``configuration.nix`` called ``nixxed.nix``:

```nix
{ config, pkgs, ... }:

let
  nixxed = pkgs.rustPlatform.buildRustPackage {
    pname = "nixxed";
    version = "0.1.0";

    src = pkgs.fetchFromGitHub {
      owner = "0x53A";
      repo = "nixxed";
      rev = "main";
      hash = "sha256-GSiuRaBuJfPh2T2PjmR3nyHRcJ28lYhW+ZGusXMeJ2U=";
    };

    cargoLock = {
      lockFile = "${pkgs.fetchFromGitHub {
        owner = "0x53A";
        repo = "nixxed";
        rev = "main";
        hash = "sha256-GSiuRaBuJfPh2T2PjmR3nyHRcJ28lYhW+ZGusXMeJ2U=";
      }}/Cargo.lock";
      allowBuiltinFetchGit = true;
    };

    nativeBuildInputs = [ pkgs.pkg-config ];
  };
in
{
  environment.systemPackages = [ nixxed ];
}
```

and then just importing it:

```nix
{ config, pkgs, ... }:

{
  imports =
    [
      ./hardware-configuration.nix
      # import the file we just created
      ./nixxed.nix
    ];

  # ...
}
```