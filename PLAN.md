# PLAN — GENESIS

> Hoja de ruta. La pregunta rectora es una sola: **¿cuál es el conjunto mínimo
> de leyes que produce complejidad sin que esta sea programada?**

Toda característica debe poder clasificarse en uno de estos tres cajones:

| Categoría | Qué es | Ejemplos |
|---|---|---|
| **Ley** (física programada) | Interacción fundamental entre partículas, sin conocimiento de especies ni de "química". | LJ, Coulomb, gravedad, choques elásticos, Verlet |
| **Observación** (instrumento) | Mide y reporta; no altera la dinámica. | g(r), agregados, histogramas, thermostat (instrumento NVT), observación de bonds |
| **Emergencia** (nunca se programa) | Lo que sale solo. Si algo parecido a "moléculas" aparece, debe ser medido, no ordenado. | condensación, bond persistence, agregados iónicos |

Un bond **nunca** es una ley de especie: es un episodio observado. El engine
solo puede acoplarlo físicamente (`enable_bond_interaction`) porque la
observación lo midió — nunca porque "el Na y el O forman NaCl".

---

## Hecho (stage 0 y 1)

- [x] ECS propio, universo, scheduler con análisis de conflictos.
- [x] Config 100% TOML tipada (serde, fail-fast) + create-on-first-run que
      **nunca pisa** una config inválida existente.
- [x] Física: LJ con mixing Lorentz–Berthelot y *switch* quíntico, choques
      elásticos con grid espacial, Verlet velocidad (conserva E), thermostat
      Berendsen opt-in.
- [x] Cargas por especie (tendencias de ionización: O −1, Na +1, Si +0.5,
      metales +1) y ley de Coulomb truncada y suavizada.
- [x] Fuerzas **paralelas deterministas** (rayon): cada par una vez, ±F a ambos
      extremos (3ª ley de Newton exacta), acumulación sparse por hilo y merge
      en orden de hilos → `E` bit-idéntico a cualquier nº de hilos.
- [x] **Observación de bonds persistentes** (`bonds.rs`): un par ligado que
      sobrevive `bond_min_periods` periodos vibracionales queda registrado con
      su vida media y su matriz por especies.
- [x] **Interacción de bonds observados** (`forces.rs`): muelle armónico hacia
      el mínimo del pozo LJ (k = curvatura del pozo), con guard de radio y
      clamp del switch (bug real corregido: r > r_c explotaba el switch).
- [x] **Agregados químicos por composición** (`bond_structure.rs`): componente
      conexa del grafo de bonds observados, etiquetada por estequiometría
      (`Na-O`, `Na2-O`, `C3`) + histograma de composiciones. La "molécula" se
      mide, nunca se programa.
- [x] `energy_total = K + V + E_bond` (la energía incluye el muelle).
- [x] Análisis escalable: g(r) con subsampleo determinista (`G_SAMPLE_CAP`),
      agregados friends-of-friends, snapshot de estructura.
- [x] Exportaciones CSV/XYZ para análisis externo (OVITO/matplotlib).
- [x] Persistencia binaria idéntica (save/reload bit-a-bit).
- [x] 10 elementos (H, He, C, N, O, Na, Si, P, S, Fe), seeding por lista.
- [x] Tests: 72/72 (`cargo test --lib`), clippy limpio.
- [x] Demos: `config/universe.toml` (100k), `config/demo-nacl.toml`
      (fusión iónica Na⁺/O⁻ observando bonds).

---

## En curso / próximos pasos

- [ ] **Ciclo de vida de especies** (stage 2): ¿puede un universo de *reglas*
      con selección de eventos raros producir organismos? Primero: métricas de
      complejidad medibles (qué observar) antes de programar nada.
- [ ] **Reacciones observadas** (no programadas): usar la lente química para
      seguir el *ciclo de vida* de una estequiometría — aparición, fusión,
      escisión, desaparición — y su energía de formación observada; sin jamás
      decir "esto es una molécula".
- [ ] **Química emergente vía tabla de porosidad/afinidad**: variar σ, ε y
      carga por especie para poblar el espacio de "materiales" que el universo
      puede formar espontáneamente.
- [ ] **Selección de eventos raros** (replicadores): definir un *evento de
      copia* como ley física de bajo costo energético y ver si alguna
      configuración se auto-sostiene. Es la frontera donde "lo que es ley" vs
      "lo que es organismo" se prueba empíricamente.
- [ ] **Análisis en vivo**: canal de datos (CSV/XYZ ya existe) + graficado
      externo automático; snapshot de estructura cada N ticks ya reportado.
- [ ] **Persistencia de bonds** en el snapshot (hoy los bonds se reconstruyen;
      guardar la historia medida para relanzar NVT con química observada).

---

## Principios de diseño (no negociables)

1. **Una vez-por-par, nunca una vez-por-átomo** para fuerzas de 2 cuerpos.
2. **Determinismo total**: mismo seed → misma historia, con cualquier nº de
   hilos. El merge acumula en orden de hilos.
3. **Física programada, química observada, biología nunca programada.**
4. **Nada de "inventar valores en runtime"**: config falla rápido con error
   claro; los defaults solo se persisten en creación.
5. **Instrumentos ≠ leyes**: el thermostat y la observación de bonds son
   medidores/acopladores opt-in; el default (NVE + observación pasiva)
   conserva energía.
