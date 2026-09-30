self: { config, lib, pkgs, ... }:
let cfg = config.services.chorus;
in {
  options.services.chorus = {
    enable = lib.mkEnableOption "Chorus federation node";
    origin = lib.mkOption { type = lib.types.str; example = "https://chorus.example.com"; };
    credentialFile = lib.mkOption { type = lib.types.str; description = "Path to an encrypted systemd credential; never a Nix store path."; };
    port = lib.mkOption { type = lib.types.port; default = 8787; };
  };
  config = lib.mkIf cfg.enable {
    users.groups.chorus = {};
    users.users.chorus = { isSystemUser = true; group = "chorus"; home = "/var/lib/chorus"; };
    systemd.services.chorus = {
      description = "Chorus federation node";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      environment = {
        CHORUS_DATA_DIR = "/var/lib/chorus";
        CHORUS_ORIGIN = cfg.origin;
        CHORUS_BIND = "127.0.0.1:${toString cfg.port}";
        CHORUS_WEB_ROOT = "${self.packages.${pkgs.stdenv.hostPlatform.system}.web}";
        CHORUS_KEY_PASSWORD_FILE = "/run/credentials/chorus.service/keystore-password";
      };
      serviceConfig = {
        ExecStart = "${self.packages.${pkgs.stdenv.hostPlatform.system}.node}/bin/chorus-node serve";
        User = "chorus"; Group = "chorus";
        StateDirectory = "chorus"; StateDirectoryMode = "0700";
        LoadCredentialEncrypted = "keystore-password:${cfg.credentialFile}";
        UMask = "0077"; Restart = "on-failure";
        NoNewPrivileges = true; PrivateTmp = true;
        ProtectSystem = "strict"; ProtectHome = true;
        MemoryMax = "2G";
      };
    };
  };
}
