THIS IS AN EXPERIMENTAL UPDATE, IF YOU ARE GOING TO USE THIS PLEASE CREATE A BACKUP OF YOUR CLIENT FOLDER AND TEST IT IN THAT. I CANNOT GUARANTEE THIS WILL WORK FOR EVERYONE'S CLIENT. THIS IS MADE FOR VERSION 0.179 I HAVE NO WAY OF KNOWING IF MY CHANGES WILL BE COMPATIBLE WITH ANY OTHER VERSION. SOME THINGS ARE MISSING/BROKEN IN VERSION 0.179 THAT MIGHT BE IN NEWER VERSIONS OR MIGHT NOT BE IN OLDER VERSIONS, AND AS A RESULT COULD BREAK YOUR GAME.

ANY FILE CREATED BY SCRIPTS WILL BE ADDED AS A LOOSE FILE IN YOUR CLIENT FOLDER, THE CLIENT AUTOMATICALLY OVERWRITES THE PACKED FILES WITH THE NEW LOOSE FILES. YOU SHOULD NOT HAVE TO EDIT ANYTHING YOURSELF UNLESS YOU ARE TRYING TO ADD YOUR OWN ARMOR SETS AND ICONS

PYTHON AND PyYAML ARE REQUIRED FOR THE SCRIPTS TO RUN 

MAKE SURE THE GAME IS NOT OPEN WHEN YOU RUN THE SCRIPTS

While this is an experimental update it is still very beginner friendly, if you can read this file you can update your client. This update contains EVERYTHING, well almost everything. I believe this update makes the game "cosmetically complete". As far as I know EVERYTHING is available in the store now.

THIS UPDATE WILL ADD:
102 armor sets and 84 single armor pieces (with a few that were never for sale in the original game)
23 Houses and all additional rooms can be spawned in and explored ( I will not be implementing building since it's already in CWA Legacy)
33 mounts
56 emotes (52 usable)
35 companions
38 companion attachments
46 holoprojectors
12 mind tricks
10 consumables
7 components
2 furniture listings
21 custom armor sets and 2 custom single armor pieces (they are lore friendly)
SCRIPTS TO CREATE YOUR OWN ARMOR SETS, STRINGS, ICONS, MODELS, AND EFFECTS (this is the fun part)

All you need to do is download Python from here: https://www.python.org/downloads/release/python-3150/
and make sure you select the option to add to PATH

Once Python is installed verify it is working correctly by opening the terminal on your desktop and running " python --version "

If python installed correctly now you can check and see if you have PIP by running " pip --version "
(it should come with python)

If pip is installed you can now run " pip install PyYAML "

After PyYAML installs you are good to run the scripts located in the tools folder of the oxide build!

Open your terminal and type " python path\to\tool.py "path\to\game_folder" "

For me it looks like " python C:\Games\rust\oxide\tools\cwa_aliases.py "C:\Games\Clone_Wars\Clone_Wars" "

After that hit enter, repeat this process for all 6 tools.

CONGRATULATIONS YOUR CLIENT IS NOW 100% UPDATED AND YOU DIDN'T HAVE TO EDIT A SINGLE THING!

Now you can build your server and check everything out, take your time exploring because a lot has been added!

PLEASE MAKE SURE TO OPEN THE README, OR THE CUSTOM .yaml AND .tsv FILES IF YOU PLAN ON ADDING YOUR OWN ENTRIES. GITHUB THROWS OFF THE LAYOUT OF MY README MAKING ENTRY EXAMPLES APPEAR WRONG.




-----------------------------------------------------------------------------------------------------------------------------------------------------


STRINGS CAN BE FOUND IN: "locale/en_us_data.dat" and "locale/en_us_data.dir"

YOU WILL CREATE NEW STRINGS IN THE "custom_strings.tsv" FILE LOCATED IN THE "config" FOLDER

CHOOSE AN ID THAT IS NOT ALREADY TAKEN AND IS LESS THAN 65535, PRESS "Tab" ONCE AND THEN ENTER THE DESIRED TEXT. ONLY ONE STRING PER LINE

RUN THE SCRIPT YOUR NEW STRINGS WILL BE ADDED TO "en_us_data.dat" AND ITS CORRESPONDING HASH WILL BE ADDED TO "en_us_data.dir" BACK IN YOUR "locale" FOLDER

I HAVE ALREADY ADDED IN CUSTOM STRINGS UNLESS YOU ARE ADDING SOMETHING NEW YOU ARE GOOD TO RUN THE SCRIPTS WITHOUT ADDING OR CHANGING ANYTHING

RUN: python path\to\cwa_locale.py "path\to\clientfolder"

-----------------------------------------------------------------------------------------------------------------------------------------------------

ALIASES ARE IDENTIFIERS USED TO DETERMINE THE TEXTURE OF AN OBJECT

YOU WILL ADD NEW ALIASES TO "custom_aliases.yaml" LOCATED IN THE CONFIG FOLDER

HERE IS HOW IT SHOULD BE FORMATTED: 

# ── ARF Sergeant Boomer ──────────────────────────────────────────────────────
- adr: Wear_Human_<gender>_Body_CloneBasicArmor.adr
  alias: SGTBoomer
  texture: Wear_Human_Male_Body_CloneBasicArmor_SGTBoomer_C.dds
  source: custom_armor/SGTBoomer_Body.dds
- adr: Wear_Human_<gender>_Hands_CloneGloves.adr
  alias: SGTBoomer
  texture: Wear_Human_Male_Hands_CloneGloves_SGTBoomer_C.dds
  source: custom_armor/SGTBoomer_Hands.dds
- adr: Wear_Human_<gender>_Feet_CloneBoots.adr
  alias: SGTBoomer
  texture: Wear_Human_Male_Feet_CloneBoots_SGTBoomer_C.dds
  source: custom_armor/SGTBoomer_Feet.dds
- adr: Wear_Human_<gender>_Head_ArfTrooperHelm.adr
  alias: SGTBoomer
  texture: Wear_Human_Male_Head_ARFTrooperHelm_SGTBoomer_C.dds
  source: custom_armor/SGTBoomer_Head.dds

PLACE YOUR CUSTOM .dds FILES IN THE "custom_armors" FOLDER

THE SCRIPT READS THE ORIGINAL ARMOR'S .adr AND CREATES A NEW ADR ADDING YOUR ALIAS TO ITS LIST OF ALIASES. THE TEXTURE YOU WANT TO BE LINKED TO THAT ALIAS NEEDS TO BE IN THE "custom_armors" FOLDER. THE TEXTURE FIELD NEEDS THE FULL NAME OF YOUR CUSTOM TEXTURE AND THE SOURCE FIELD TELLS IT WHERE TO LOOK FOR THAT TEXTURE. ADDING THE "gender" MODIFIER TO THE .adr FIELD WILL MAKE THE ALIAS APPLY FOR MALE AND FEMALE PLAYERS

I HAVE ALREADY ADDED IN CUSTOM TEXTURES UNLESS YOU ARE ADDING SOMETHING NEW YOU ARE GOOD TO RUN THE SCRIPTS WITHOUT ADDING OR CHANGING ANYTHING

RUN: python path\to\cwa_aliases.py "path\to\clientfolder"

--------------------------------------------------------------------------------------------------------------------------------------------------------

OFFHAND HILTS IS A PATCH I MADE TO ALLOW YOU TO USE ANY LIGHTSABER IN YOUR OFFHAND

THE SCRIPT WILL USE THE DATA FROM LIGHTSABERS ALREADY STORED IN YOUR GAME TO CREATE AN OFFHAND COPY USING THE SAME MODELS JUST ATTACHED TO THE LEFT HAND.

THERE IS NOTHING YOU NEED TO DO FOR THIS SCRIPT AS THE DATA COMES FROM THE LIGHTSABERS YOU ALREADY HAVE, YOU CAN RUN IT WITHOUT CHANGING ANYTHING

PURCHASING A LIGHTSABER HILT WILL ADD AN ADDITIONAL "(LEFT)" HANDED VARIANT OF THE HILT TO YOUR INVENTORY, IF YOU ALREADY OWN THE NORMAL HILT THE SERVER WILL NOTICE THIS AND GIVE YOU THE "(LEFT)" VARIANT ON LOGIN.

RUN: python path\to\cwa_offhand_hilts.py "path\to\clientfolder"

---------------------------------------------------------------------------------------------------------------------------------------------------------


SPAWNING SOME MODELS AS A COMPANION CAUSES THEM TO BE INVISIBLE, THE WORKAROUND FOR THIS IS PATCHING THEIR .agr FILE. BY CHANGING ValidatePcNpc= TO 0 THE COMPANION WILL SKIP THE PC/NPC CHECK AND SPAWN NORMALLY
THIS SCRIPT CAN ALSO BE USED TO BUILD MARKETPLACE MODELS FOR YOUR CUSTOM ARMOR SETS

NEW .agr FILES ARE BUILT FROM ENTRIES IN "custom_models.yaml" FOUND IN THE CONFIG FOLDER

HOW TO FORMAT ARMOR SETS:

# Clone Gunner
- id: 2905
  file: Grp_Clone_CloneTrooperGunner.agr
  race: "1"
  description: Marketplace Model
  gender: "1"
  actors:
    - {model: Char_HumanMaleBody.adr, validate: 1, aliases: [CloneTan]}
    - {model: Char_CloneHead.adr, usage: 3, slot: CustomizationHead, aliases: [Whiskers, CloneTan]}
    - {model: Wear_Human_Male_Body_CloneGunnerArmor.adr, usage: 2, validate: 1, slot: Body, slots: true, aliases: [GunnerBlue]}
    - {model: Wear_Human_Male_Feet_CloneBoots.adr, usage: 2, slot: Feet, slots: true, aliases: [GunnerBlue]}
    - {model: Wear_Human_Male_Hands_CloneGloves.adr, usage: 2, slot: Hands, slots: true, aliases: [GunnerBlue]}
    - {model: Wear_Human_Male_Head_CloneGunnerHelm.adr, usage: 2, slot: Head, slots: true, aliases: [GunnerBlue]}
    - {model: Wield_BlasterRifle_DC15S.adr, usage: 3, validate: 1, slot: Weapon, slots: true, aliases: [CloneBasic]}



I HAVE ALREADY ENTERED ALL COMPANIONS AND MISSING ARMORS AND MADE SURE THEY WORK PROPERLY ALL YOU NEED TO DO IS RUN THE SCRIPT 

RUN: python path\to\cwa_models.py "path\to\clientfolder"

----------------------------------------------------------------------------------------------------------------------------------------------------------


ICON DATA IS STORED IN 3 FILES IN YOUR ASSET PACKS: "Images.txt", "ImageSets.txt", and "ImageSetMappings.txt"

YOU WILL ADD THE .dds FILES FOR YOUR CUSTOM ICONS TO THE "custom_icons" FOLDER

THEN YOU WILL GO TO "custom_icons.yaml" TO ADD YOUR CUSTOM ICON ENTRIES

HERE IS HOW IT SHOULD BE FORMATTED:

- icon_set_id: 3873
  image_id: 7067
  file: Icon_Item_Char_HodgePodgeDroid.dds

THE SCRIPT WILL THEN REBUILD "Images.txt", "ImageSets.txt". "ImageSetMappings.txt" ADDING IN THE INFORMATION SPECIFIED IN "custom_icons.yaml"

I HAVE ALREADY ADDED CUSTOM ICONS UNLESS YOU ARE ADDING SOMETHING NEW YOU DO NOT NEED TO MAKE ANY CHANGES

RUN: python path\to\cwa_icons.py "path\to\clientfolder"


-----------------------------------------------------------------------------------------------------------------------------------------------------------

COMPANIONS, LIGHTSABER CUSTOMIZATION, EMOTES, MIND TRICKS, HOLOPROJECTORS, AND THE SHOPS ARE FULLY FUNCTIONAL

PROFILE FUNCTIONALITY PARTIALLY ADDED

EXTRACT, CARGO BUILD, RUN

Patch Contains WexToxicstinger's shop implementations and my shop npcs

Thank you to Delta-07 for making the oxide build that I referenced to get the actions menu working.

next goals: add ambient npcs to the rest of the temple, implement profile tab
