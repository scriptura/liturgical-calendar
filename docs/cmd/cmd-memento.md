# Commandes standard

```bash
cargo build -p liturgical-calendar-core
cargo test -p liturgical-calendar-core
cargo clippy -p liturgical-calendar-core -- -D warnings
cargo tree -p liturgical-calendar-core
```

```bash
cargo build -p liturgical-calendar-forge
cargo test -p liturgical-calendar-forge
cargo clippy -p liturgical-calendar-forge -- -D warnings
cargo tree -p liturgical-calendar-forge
```

Tout builder :

```bash
cargo build --workspace
```

Tout tester :

```bash
cargo test --workspace
```

Lancer clippmy sur le projet :

```bash
cargo clippy --workspace
```

Publier les crates sur crates.io (installation de cargo-release nécessaire) :

```bash
cargo publish --workspace
```

Forger les binaires `.kald` + `.lits` ensembles (pour éviter une désynchronisation) :

```bash
cargo run -p liturgical-calendar-forge --bin kal-forge -- \
    --rite romanus \
    --scope universale \
    --corpus ./corpus \
    --out ./artifacts \
    --i18n
```

---

Lister les entrées du 20 janvier 2026 (doy 19) :

```bash
cargo run -q -p liturgical-calendar-forge --bin kal-read -- \
    --kald ./artifacts/romanus_universale.kald \
    --lits ./artifacts/romanus_universale_la.lits \
    --year 2026 --doy 19
```

Lister tous les jours de l'année 2026, uniquement doy + date + label :

```bash
for doy in $(seq 0 365); do
    [ "$doy" -eq 59 ] && continue # Padding Entry en années non-bissextiles

    if [ "$doy" -lt 59 ]; then
        offset=$doy
    else
        offset=$((doy - 1))
    fi

    date_str=$(LC_TIME=C date -d "2026-01-01 +$offset days" +"%d %b" | tr '[:upper:]' '[:lower:]')

    labels=$(cargo run -q -p liturgical-calendar-forge --bin kal-read -- \
                  --kald ./artifacts/romanus_universale.kald \
                  --lits ./artifacts/romanus_universale_la.lits \
                  --year 2026 --doy $doy 2>/dev/null |
             awk '
                 /^[[:space:]]*label[[:space:]]*:/ {
                     sub(/^[^:]*:[[:space:]]*/, "")
                     main = $0
                 }
                 /^\[Padding/ {
                     main = $0
                 }
                 /^[[:space:]]+\[[0-9]+\][[:space:]]/ && !/feast_id/ {
                     sub(/^[[:space:]]+\[[0-9]+\][[:space:]]*/, "")
                     if ($0 != "") secondary[++s] = $0
                 }
                 END {
                     printf "%s", main
                     for (i=1; i<=s; i++) printf " | %s", secondary[i]
                 }
             ')

    printf '%3d  %s  %s\n' "$doy" "$date_str" "$labels"
done
```

Lister toutes les fêtes du 1 au 31 janvier 2026, avec toutes leur infos :

```bash
for doy in $(seq 0 30); do
    echo -n "$(printf '%3d' $doy)  "
    cargo run -q -p liturgical-calendar-forge --bin kal-read -- \
        --kald ./artifacts/romanus_universale.kald \
        --lits ./artifacts/romanus_universale_la.lits \
        --year 2026 --doy $doy \
        | grep -E "label|annotation|feast_id|precedence|nature|color|\["
done
```

Lister tous les dimanches de l'année 2026 (commence à doy 3), uniquement doy + label :

```bash
doy=3
while [ $doy -le 365 ]; do
    if [ $doy -ne 59 ]; then
        label=$(cargo run -q -p liturgical-calendar-forge --bin kal-read -- \
            --kald ./artifacts/romanus_universale.kald \
            --lits ./artifacts/romanus_universale_la.lits \
            --year 2026 --doy $doy 2>/dev/null \
            | grep -v '^\s*\[' \
            | grep "label" \
            | sed 's/^.*label\s*:\s*//')
        printf '%3d  %s\n' $doy "$label"
    fi
    doy=$(( doy + 7 ))
    [ $doy -eq 59 ] && doy=60 # Padding Entry en années non-bissextiles
done
```

Utiliser notre script shell pour visualiser toutes les labels d'une année :

```bash
./calendar_md.sh 2026
```

Idem avec création de markdowns consultables

```bash
./calendar_md.sh 2026 > calendar_2026.md
```

Générer des fichiers Markdown pour plusieurs années :

```bash
for y in 1973 1974 1976 1984 1999 2000 2008 2009 2010 2020 2021 2022 2023 2024 2025 2026 2027 2057; do
./calendar_md.sh "$y" > "./docs/outputs/calendar_${y}.md"
done
```

Générer une plage continue (par exemple de 2025 à 2030) :

```bash
for y in {2025..2030}; do
./calendar_md.sh "$y" > "./docs/outputs/calendar_${y}.md"
done
```
