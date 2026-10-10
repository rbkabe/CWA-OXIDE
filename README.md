THIS IS AN EXPERIMENTAL UPDATE, IF YOU ARE GOING TO USE THIS PLEASE CREATE A BACKUP OF YOUR CLIENT FOLDER AND TEST IT IN THAT. I CANNOT GUARANTEE THIS WILL WORK FOR EVERYONE'S CLIENT. THIS IS MADE FOR VERSION 1.79 I HAVE NO WAY OF KNOWING IF MY CHANGES WILL BE COMPATIBLE WITH ANY OTHER VERSION. SOME THINGS ARE MISSING/BROKEN IN VERSION 1.79 THAT MIGHT BE IN NEWER VERSIONS OR MIGHT NOT BE IN OLDER VERSIONS, AND AS A RESULT COULD BREAK YOUR GAME.

ANY FILE CREATED BY THE SCRIPTS WILL BE ADDED AS A LOOSE FILE IN YOUR CLIENT FOLDER, THE CLIENT AUTOMATICALLY OVERWRITES THE PACKED FILES WITH THE NEW LOOSE FILES. YOU SHOULD NOT HAVE TO EDIT ANYTHING YOURSELF UNLESS YOU ARE TRYING TO ADD YOUR OWN ARMOR SETS AND ICONS

PYYAML IS REQUIRED FOR THE SCRIPTS TO RUN 

THE SCRIPTS MUST BE TOLD WHERE TO LOOK

START IN YOUR BACKED UP CLIENT FOLDER

OPEN CLIENT FOLDER IN TERMINAL

RUN: python path\to\pythontool "path\to\clientfolder"


-----------------------------------------------------------------------------------------------------------------------------------------------------


STRINGS CAN BE FOUND IN: "locale/en_us_data.dat" and "locale/en_us_data.dir"

YOU WILL CREATE NEW STRINGS IN THE "custom_strings.tsv" FILE LOCATED IN THE "config" FOLDER

CHOOSE AN ID LESS THAN 65535 PRESS "Tab" ONCE AND THEN ENTER THE DESIRED TEXT. ONLY ONE STRING PER LINE

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

THE SCRIPT READS THE ORIGINAL ARMOR'S .adr AND ADDS YOUR NEW ALIAS TO ITS LIST OF ALIASES. THE TEXTURE FIELD NEEDS THE FULL NAME OF YOUR CUSTOM TEXTURE AND THE SOURCE FIELD TELLS IT WHERE TO LOOK FOR THAT TEXTURE. ADDING THE <gender> MODIFIER TO THE .adr FIELD WILL MAKE THE ALIAS APPLY FOR MALE AND FEMALE PLAYERS

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

NEW .agr FILES ARE BUILT FROM ENTRIES IN "custom_models.yaml" FOUND IN THE CONFIG FOLDER

I HAVE ALREADY ENTERED ALL COMPANIONS AND MADE SURE THEY WORK PROPERLY ALL YOU NEED TO DO IS RUN THE SCRIPT 

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

at this point I think I have every single emote fully functioning besides a few, all holoprojectors and mind tricks are there.

next goals: add ambient npcs to the rest of the temple, implement profile tab
