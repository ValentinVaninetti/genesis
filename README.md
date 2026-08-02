# GENESIS

> Motor de simulación de un universo. **No** un simulador de química. **No** un
> simulador de biología. Un motor cuyo objetivo es programar únicamente las
> reglas fundamentales y observar si la complejidad emerge espontáneamente.

La única pregunta que intenta responder:

> ¿Cuál es el conjunto mínimo de leyes necesario para que aparezca complejidad
> sin haber sido programada?

Si algún día aparece algo parecido a la vida, debe ser una consecuencia de las
reglas, **nunca** una característica implementada explícitamente. Este README
documenta las decisiones de arquitectura de la etapa 0 (la base).

---

## Estado actual

- ✅ ECS propio y completo (arquetipos, generaciones, consultas paralelas).
- ✅ Universo (tiempo, RNG serializable, recursos, estadísticas).
- ✅ Scheduler explícito con análisis de etapas/conflictos.
- ✅ Configuración 100% en TOML, fuera del código.
- ✅ Persistencia binaria: guardar y retomar universos idénticos.
- ✅ Componentes base (posición, velocidad, masa, carga, tipo atómico).
- ✅ **Colisión elástica** entre partículas (impulso; conserva momento y
     energía por par), con **grid espacial** uniforme como broad-phase.
- ✅ **Fuerzas de Lennard-Jones** entre partículas (átomo de cada elemento con
     σ y ε de fase condensada): repulsión r⁻¹² + atracción r⁻⁶, con mezcla de
     Lorentz–Berthelot, *switch* quíntico a 0.9·r_c y núcleo endurecido.
- ✅ **Integrador velocity Verlet** (simpéctico): conserva la energía total
     (cinética + potencial) al nivel del esquema numérico.
- ✅ **Energía, temperatura y potencial derivados** (no se almacenan como
     componentes): `E = K + V`, temperatura por equipartición.
- ✅ **Siembra en red cúbica** + jitter térmico cuando hay fuerzas (estándar de
     dinámica molecular): un sembrado aleatorio superpone núcleos y la
     repulsión r⁻¹² explota numéricamente.
- ✅ Observado: a baja temperatura el gas **se condensa** espontáneamente (V se
     vuelve muy negativo, el sistema se autocalienta con el calor latente).
- ✅ **Análisis de estructura emergente** (observación, no leyes): g(r) con
     normalización de gas ideal y detección de agregados con friends-of-friends
     (`src/analysis/`). `StructureSystem` muestrea cada `stats.structure_interval`
     ticks y el snapshot reporta agregados/monómeros/mayor/pares ligados.
- ⏳ Visualización: **fuera del motor** (solo consola hoy).

```
cargo run --release [config.toml] [ticks] [reportar_cada]
cargo test
```

---

## Decisiones de arquitectura (y por qué)

### 1. ECS propio, no una librería

`bevy_ecs`/`specs` traen décadas de diseño, pero acoplan el proyecto a un
sistema de tipos ajeno, con errores de compilación difíciles y una migración
costosa. Un ECS propio (~600 líneas) da control total, cero dependencias
frágiles y la libertad de evolucionar durante años. Todo el código unsafe está
**ausente**: los downcasts se validan contra un registro global de tipos.

### 2. Arquetipos (no sparse-sets sueltos)

Todos los átomos comparten el mismo set de componentes → viven en arrays
contiguos SoA (`Vec<T>` por componente). Esto garantiza:

- **Localidad de caché**: iterar 10M de posiciones recorre memoria contigua.
- **Alineación natural**: la fila `i` es la misma entidad en todas las columnas,
  lo que permite paralelizar por chunks sin riesgo de desalineación.
- **Crecimiento heterogéneo**: cuando aparezcan fotones, moléculas u organismos
  con sets distintos, cada tipo vivirá en su propio arquetipo. No hay costo por
  entidades "vacías".

### 3. `EntityId` generacional

Los ids nunca se reutilizan sin incrementar la generación. Un handle "zombie"
(salvado hace semanas, o de un `Bonds` que apunta a una entidad destruida)
devuelve `None` en lugar de leer datos corruptos. Es la diferencia entre un
crash sutil y una simulación que degrada con gracia.

### 4. Los componentes son datos puros

`Component` solo exige `Send + Sync + 'static` y un `ComponentId` estable.
Sin `Clone` obligatorio, sin herencia, sin lógica. Las entidades **no** tienen
métodos de comportamiento: el comportamiento vive en los `System`.

### 5. Los sistemas declaran su acceso

Cada `System` declara qué componentes/recursos lee y escribe. El scheduler:

- ejecuta en **orden explícito de registro** (nada oculto),
- calcula **etapas** sin conflictos (que hoy se ejecutan secuencialmente y en
  el futuro se paralelizarán con préstamos disjuntos),
- expone el plan para inspección y tests.

El paralelismo real de hoy vive **dentro** de cada sistema: consultas
`par_for_each*_mut` con rayon, exactamente donde el cuello de botella ocurre
en simulación de partículas.

### 6. El `Universe` es la única fachada

Contiene todo: tiempo, RNG, `World`, recursos, scheduler, estadísticas y
configuración. La API pública es diminuta (`new`, `tick`, `run_ticks`, `save`,
`load`). Los sistemas **nunca** ven el `Universe` completo: solo un
`SystemContext` acotado, lo que impide el acoplamiento accidental.

### 7. La persistencia es un snapshot total

Guarda configuración + reloj + **estado del RNG** + estadísticas + todas las
entidades con sus ids byte a byte. Un universo guardado retoma con la misma
secuencia aleatoria y los mismos handles. Formato binario (`bincode`).

### 8. La configuración es el "big bang"

Los únicos momentos en que el código crea materia son: (a) el sembrado inicial
según `config/universe.toml`, y (b) la restauración de snapshots. A partir de
ahí, **todo** debe emerger de las leyes.

### 9. Física/química como espacios delimitados

`physics/` ya contiene leyes reales (colisión elástica, grid espacial);
`chemistry/` documenta la regla de oro: ninguna ley física puede depender de
química; ninguna "reacción" se programa como tal. Son la frontera que protege
la pregunta del proyecto.

---

## Estructura

```
src/
├── main.rs              # CLI: config + ticks + demo de persistencia
├── lib.rs
├── universe/            # Universe (fachada), Time, siembra inicial
├── ecs/                 # entity, component, archetype, world, resource
├── components/          # catálogo único + registro (macro for_each_component!)
├── systems/             # leyes: movement, boundaries, collisions, forces, integrate
├── scheduler/           # System trait, Access, Scheduler, etapas
├── config/              # Config tipada (TOML)
├── serialization/       # Snapshot total (bincode)
├── stats/               # métricas + historial
├── analysis/            # lentes: g(r) y agregados (observación, no leyes)
├── math/                # Vec3
├── rng/                 # RNG serializable (xoshiro256++)
├── physics/             # leyes: colisión elástica, LJ (tablas y switch), grid
├── chemistry/           # reservado: documenta la prohibición
```

---

## Agregar un componente nuevo (1 minuto)

1. Crear `src/components/<nuevo>.rs` con `impl Component for Nuevo { const ID }`.
2. Añadirlo a la macro `for_each_component!` en `src/components/mod.rs`.
   - Serialización, registro y snapshots se generan solos.

> ⚠️ Los `ComponentId` son permanentes: jamás reasignar ni reutilizar.

## Agregar una ley nueva

1. Implementar `System` (name + `access()` + `run`).
2. Registrarla en `build_schedule` (`src/universe/mod.rs`), activable desde
   `[systems]` en TOML.

---

## Límites honestos

- El scheduler calcula etapas paralelas pero hoy ejecuta secuencialmente; la
  paralelización global requiere una abstracción de *borrows disjuntos* que se
  agregará cuando haya más de un sistema de cómputo pesado.
- Las fuerzas internas son de **esferas de Lennard-Jones** sin conservación de
  identidad: un choque a gran velocidad entre dos átomos los repele, pero no
  hay "romper un enlace" porque los enlaces no se representan (todavía).
- El broad-phase de fuerzas usa el mismo grid uniforme que las colisiones; a
  densidades extremas el factor de cutoff podría reajustarse.
- Las colisiones elásticas y las fuerzas coexisten de forma independiente
  (activables por separado en `[systems]`); no hay aún un formalismo unificado
  de ambos.
- La iteración paralela de dos componentes asume sets homogéneos por arquetipo
  (garantizado por diseño: dentro de un arquetipo todas las filas están
  alineadas).
- Las métricas de memoria son aproximaciones del propio ECS (capacidades).
