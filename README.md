# Vertex Tools
Where all of the tools like the updater and package manager are held to grab for updates

# Todo:
- Fix the AUR not working at ALL on vpkg and just throwing erros about how yay does not exsist. (Use our own AUR builder instead)
- Rewrite VPKG to allow all 3 package manager's packages to be installed with one command, so instead of spesifying flatpak or pacman, it would let you install packages from any of the 3 at onece, and if there were many instances of that package you were trying to install on all of the managers it would ask you where to install it from. So instead of sudo vpkg pm install prismlauncher it would be sudo vpkg install prismlauncher