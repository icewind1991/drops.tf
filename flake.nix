{
  inputs = {
    nixpkgs.url = "nixpkgs/nixos-24.11";
    flakelight = {
      url = "github:nix-community/flakelight";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    mill-scale = {
      url = "github:icewind1991/mill-scale";
      inputs.flakelight.follows = "flakelight";
    };
  };
  outputs = {mill-scale, ...}:
    mill-scale ./. {
      extraPaths = [
        ./.sqlx
        ./benches
        ./templates
      ];
      withOverlays = [
        (import ./nix/overlay.nix)
      ];
      nixosModules = {outputs, ...}: {
        default = {
          pkgs,
          config,
          lib,
          ...
        }: {
          imports = [./nix/module.nix];
          config = lib.mkIf config.services.dropstf.enable {
            nixpkgs.overlays = [outputs.overlays.default];
            services.dropstf.package = lib.mkDefault pkgs.dropstf;
          };
        };
      };
    };
}
