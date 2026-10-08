## Descargar · Download

| Sistema · System | Archivo · File |
| --- | --- |
| macOS 12 o posterior (Apple Silicon e Intel) | `Fino_x.y.z_universal.dmg` |
| Windows 10 / 11 (64 bits) | `Fino_x.y.z_x64-setup.exe` |

> [!WARNING]
> **Versión sin firma.** Fino todavía no está firmado con un certificado de Apple ni de Windows, así que la primera vez el sistema avisa de que no puede verificar al desarrollador. Es esperado; el código es abierto y cada archivo lleva su suma SHA-256 en `SHA256SUMS.txt`.
>
> **Unsigned build.** Fino is not yet signed with an Apple or Windows certificate, so the first launch warns that the developer can't be verified. That is expected; the source is open and every file's SHA-256 is listed in `SHA256SUMS.txt`.

### macOS

1. Abre el `.dmg` y arrastra **Fino** a **Aplicaciones**.
2. Abre Fino. Cuando macOS diga que no puede comprobarlo, cierra el aviso.
3. Ve a **Ajustes del Sistema → Privacidad y seguridad**, baja hasta «Se bloqueó Fino» y pulsa **Abrir igualmente**. Solo hace falta una vez.

*Open the `.dmg`, drag Fino to Applications and open it. When macOS says it can't verify it, close the warning, then go to System Settings → Privacy & Security and click **Open Anyway**. Only needed once.*

### Windows

1. Ejecuta `Fino_x.y.z_x64-setup.exe`.
2. Si aparece **«Windows protegió su PC»**, pulsa **Más información → Ejecutar de todas formas**.
3. Fino se instala solo para tu usuario (sin pedir permisos de administrador) en `%LOCALAPPDATA%\Programs\Fino`.

*Run the installer. If "Windows protected your PC" appears, click More info → Run anyway. Fino installs for your user only, no admin rights needed.*

### Verificar la descarga · Verify the download

```sh
shasum -a 256 -c SHA256SUMS.txt --ignore-missing      # macOS
```

```powershell
Get-FileHash .\Fino_*_x64-setup.exe -Algorithm SHA256  # Windows
```
