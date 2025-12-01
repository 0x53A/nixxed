# Example NixOS configuration for testing nixxed

{ config, pkgs, ... }:

{
  # Basic system settings
  networking.hostName = "nixos-test";
  time.timeZone = "Europe/Berlin";

  # Programs with simple enable
  programs.git = {
    enable = true;
    lfs = "asdfg";
  };
  programs.vim.enable = true;
  programs.zsh.enable = false;

  # Programs with extra configuration
  programs.neovim = {
    enable = true;
    defaultEditor = true;
    viAlias = true;
  };

  # Services with simple enable
  services.openssh.enable = true;
  services.printing.enable = true;

  # Services with extra configuration
  services.nginx = {
    enable = true;
    recommendedGzipSettings = true;
    virtualHosts."example.com" = {
      root = "/var/www/example";
    };
      additionalModules = [a, b];
  };

  # System packages
  environment.systemPackages = with pkgs; [
    wget
    curl
    htop
    ripgrep
    fd
    bat
    # eza
    git
  ];

  # Users
  users.users.test = {
    isNormalUser = true;
    extraGroups = [ "wheel" ];
  };
}
