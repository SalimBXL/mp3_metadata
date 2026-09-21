# Changelog

Toutes les modifications notables de ce projet sont documentées dans ce
fichier.

Le format suit [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/),
et ce projet adhère au [versionnage sémantique](https://semver.org/lang/fr/).

L'historique antérieur à ce fichier n'est pas reconstitué ici ; voir
l'historique Git pour les commits précédents.

## [0.2.0] - 2026-09-21

### Ajouté

- Lecture du tag ID3v1 / ID3v1.1, situé dans les 128 derniers octets du
  fichier, indépendamment du tag ID3v2 (`Id3v1Tag`, `read_id3v1_tag`,
  nouveau module `src/id3v1.rs`).
- Affichage côte à côte des tags ID3v2 et ID3v1 dans le CLI lorsque les
  deux sont présents (`side_by_side` dans `main.rs`), avec les champs
  communs (`Title` à `Genre`) dans le même ordre des deux côtés pour
  qu'ils s'alignent ligne à ligne. Version et nombre de frames figurent
  désormais dans le titre de la section ID3v2 plutôt que sur des lignes
  séparées.
- Genres ID3v1 étendus : reconnaissance de l'extension Winamp (index 80 à
  191), en plus des 80 genres d'origine. L'affichage marque d'un `*` tout
  genre hors des 80 d'origine (nom Winamp ou index brut si vraiment
  inconnu), pour distinguer d'un coup d'œil un genre "officiel" ID3v1 d'un
  genre seulement conventionnel.
- Recherche MusicBrainz par album (`search_by_album` dans `src/verify.rs`),
  en complément de la recherche par morceau existante : pour un titre très
  repris en concert ou très réédité, où des centaines d'enregistrements
  distincts partagent le même score de pertinence maximal, chercher
  directement l'édition locale évite de la perdre dans la masse. Le
  rapport obtenu par cette voie, marqué `via_album` (colonne "Voie" dans
  le tableau du CLI), est placé en tête des résultats de `verify_tag`.
- Durée audio exacte à partir d'un en-tête Xing/Info ou VBRI trouvé dans
  la première frame (`AudioFormat::duration_is_exact`,
  `AudioFormat::average_bitrate_kbps`), plutôt qu'estimée à débit
  constant. L'affichage précède la durée d'un `~` quand elle n'est
  qu'une estimation, et montre le débit moyen réel plutôt que celui de
  la seule première frame quand il est connu.
- Prise en charge de l'unsynchronisation propre à une frame individuelle
  (ID3v2.4 uniquement, bit `n` des format flags), en plus de celle,
  déjà gérée, du tag entier.
- Tests unitaires pour `Mp3Error` (`Display` de chaque variante,
  `source()`), jusque-là dépourvu de toute couverture.
- Documentation manquante complétée sur l'ensemble du crate (52 éléments
  publics sans commentaire `///`, détectés via `#![warn(missing_docs)]`) :
  `Mp3Error` et ses variantes, les enums de `mpeg.rs`, plusieurs champs de
  `Id3v1Tag`/`Id3Version`/`Id3v2Tag`/`FrameContent`, et une doc de crate en
  tête de `lib.rs`.

### Modifié

- `Id3v2Tag::Display` réordonné (`Title`, `Artist`, `Album`, `Year`,
  `Comment`, `Track`, `Genre`, puis `Album Artist` et `Cover`, propres à
  ID3v2) pour correspondre à l'ordre de `Id3v1Tag::Display`.
- `Id3v1Tag::Display` affiche désormais toujours la ligne `Track` (avec
  `?` en son absence) plutôt que de l'omettre pour un tag ID3v1 classique,
  afin de ne pas décaler `Genre` par rapport à son vis-à-vis ID3v2.
- README mis à jour au fil des évolutions ci-dessus : structure du
  projet, table de l'API, exemples d'utilisation et de sortie du CLI
  (recopiés depuis une exécution réelle), section des fonctionnalités.

### Corrigé

- README : exemple d'utilisation et de sortie du CLI qui ne
  correspondaient plus à l'API actuelle (référençaient un ancien
  `mp3_file.header` et une fonction `read_frames` libre, tous deux
  disparus depuis) ; mentions obsolètes sur le type MIME des images
  `APIC` (en réalité déjà extrait) et le décodage des frames `COMM` (en
  réalité déjà correct) dans les limitations connues.
