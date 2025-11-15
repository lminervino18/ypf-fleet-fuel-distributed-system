# YPF Ruta
Enunciado disponible en [este enlace](https://concurrentes-fiuba.github.io/2025_2C_tp2.html) o bien su copia local en [docs/enunciado_tp2](docs/enunciado_tp2.pdf).

## Integrantes
- **Nombre del equipo**: `:(){ :|:& };:`
- **Corrector**: Darius Maitia

| Nombre              | Padrón  |
|---------------------|---------|
| [Alejo Ordonez](https://github.com/alejoordonez02) | 108397 |
| [Francisco Pereyra](https://github.com/fapereyra) | 105666 |
| [Lorenzo Minervino](https://github.com/lminervino18) | 107863 |
| [Alejandro Paff](https://github.com/AlePaff) | 103376 |


## Levantar proyecto
Ejecutar el siguiente comando en la terminal para ejecutar el lado del servidor
```bash
./scripts/launch.sh <N_replicas> <M_estaciones>
```

Y para el administrador de tarjetas (cliente) se debe ejecutar por ejemplo:
```bash
# especificar el servidor, en caso contrario utiliza el default
./scripts/admin.sh --server localhost:7070 limit-account --amount 100.50

# Limitar los montos disponibles en la cuenta
./scripts/admin.sh limit-account --amount 100.50

# Limitar el monto disponible de una tarjeta en particular
./scripts/admin.sh limit-card --card-id card123 --amount 50.0

# Consultar el saldo de la cuenta
./scripts/admin.sh query-account

# Consultar los saldos de las tarjetas de la cuenta
./scripts/admin.sh query-cards

# Realizar la facturación de la cuenta
./scripts/admin.sh bill --period 2025-10
```


## Informe
El informe que se presenta a continuación está disponible en formato PDF $\LaTeX{}$ en [docs/informe.pdf](docs/informe/informe.pdf).

## Introducción
En este trabajo se desarrolla **YPF Ruta**, un sistema que permite a las empresas centralizar el pago y el control de gasto de combustilble para su flota de vehículos.  
Las empresas tienen una cuenta principal y tarjetas asociadas para cada uno de los conductores de sus vehículos. Cuando un vehículo necesita cargar en cualquiera de las 1600 estaciones distribuídas alrededor del país, puede utilizar dicha tarjeta para autorizar la carga; siendo luego facturado mensualmente el monto total de todas las tarjetas a la compañía.

## Aplicaciones
### Server
El servidor consiste de un sistema distribuido en el que existen tres tipos diferentes de clústers de nodos:

- *Surtidores* en una estación.
- *Nodos suscriptos a una tarjeta*.
- *Nodos líderes de tarjetas* que forman una *cuenta*.

Entidades que participan:

- **Surtidores.** Los surtidores corresponden a las máquinas interconectadas de manera *local* en una estación.
- **Estaciones/Nodos.** Los nodos representan estaciones de YPF. Dentro de una estación, uno de los surtidores tiene la responsabilidad de llevar a cabo la función del nodo en el sitema global.

Hay tres tipos de nodos:

- **Suscriptor (tarjeta).** Los nodos suscriptores mantienen informados a sus pares (otros nodos suscriptos a la misma tarjeta) sobre las actualizaciones al registro de la tarjetas a la que suscriben. Un nodo puede estar suscripto a varias tarjetas.
- **Líder (tarjeta).** Los nodos líder *lideran* un clúster de nodos suscriptores a una tarjeta; ésto es: tienen la responsabilidad de intercomunicar a los nodos del clúster y a su vez de informar sobre actualizaciones de la tarjeta al *nodo cuenta* cuando este así lo solicite. Un nodo líder es también un nodo suscriptor.
- **Cuenta.** Los nodos cuenta se comunican con un nodo líder de cada una de las tarjetas que le pertenecen a la cuenta. Un nodo cuenta **no** puede ser el líder de un clúster de nodos suscriptos a una tarjeta.

### Cliente
El único cliente (fuera del servidor de YPF) es el **administrador**. El administrador puede

- Limitar los montos disponibles en su cuenta.
- Limitar los montos disponibles en las tarjetas de la cuenta.
- Consultar los saldos de las cuentas.
- Consultar los saldos de las tarjetas de la cuenta.
- Realizar la facturación de la cuenta.

## Arquitectura del servidor
Como ya se mencionó, el servidor está implementado de manera distribuida. El foco principal del diseño de la arquitectura está en reducir la cantidad de mensajes entre nodos que tienen viajar en la red, partiendo de la arquitectura trivial: un grafo completo, con réplicas de la información del sistema en todos los nodos.  

Se pueden hacer varias optimizaciones a partir de algunas observaciones del *modelo de negocio* del sistema. Existe localidad con respecto al posicionamiento geográfico de las estaciones; un conductor que aparece en una estación probablemente vuelva a aparecer en estaciones cercanas, y probablemente no aparazca en una estación en la otra punta del país (o al menos no con frecuencia significativa).  
Una forma de optimizar la comunicación entre nodos sería entonces tenerlos separados por cuentas: cada nodo tendría una réplica de la información de todas las tarjetas de la cuenta a la que pertenece y sólo debería comunicar a los otros nodos del clúster de la cuenta respecto de las actualizaciones de la misma.  
El problema con esto último es que una empresa grande, con muchas tarjetas y muchos conductores a lo largo del país; tendría réplicas innecesarias: un conductor que vive en Salta probablemente no use una estación en Santa Cruz, sin embargo, si uno de sus compañeros de trabajo así lo hace, entonces el registro de su tarjeta estaría replicado en la estación de Santa Cruz.  
La solución que se encontró es la de dividir los clústers por tarjeta y no por cuenta. Ahora bien, como también necesitamos centralizar la información de todas las tarjetas pertenecientes a una cuenta, surge la necesidad de los nodos *cuenta*. Para minimizar la comunicación de los nodos cuenta con los nodos de las tarjetas que le pertenecen, el rol de comunicador se centraliza en los nodos *líder tarjeta*.  

### Tipos de clúster
A continuación se explican más en profundidad cada uno de los tipos de clúster que se mencionaron.

#### Clúster de surtidores.

Los surtidores en una estación están conectados de manera local y se encargan de mantener actualizado al surtidor líder del clúster para que este ejerza la función de nodo estación en el sistema global.

![Dos estaciones, con cuatro surtidores cada una.](doc/informe/diagrams/station-cluster-overview.svg)

#### Clúster de nodos suscriptos a una tarjeta.

Los nodos suscriptos a una tarjeta informan a sus pares de las actualizaciones en los registros de las tarjetas a las que suscriben. Hay un líder del clúster y los *súbditos* se encargan de elegirlo al principio de la ejecución y en caso de que el mismo deje de estar activo.

![Clúster de nodos suscriptos a una tarjeta.](doc/informe/diagrams/card-cluster-overview.svg)

#### Clúster de cuenta.

El clúster de nodos líderes de tarjetas tienen su propio líder: el *nodo cuenta*. Dentro de éste clúster se mantiene actualizado al nodo cuenta ante cualquier cambio en alguno de los registros de las tarjetas que conforman la cuenta. Los *súbditos* eligen un líder al principio de la ejecución y en caso de que el mismo deje de estar activo. Las actualizaciones son comunicadas sólo cuando el nodo cuenta así lo solicita.

![Clúster de cuenta.](doc/informe/diagrams/account-clusters-overview.svg)

### Vista de águila

Cabe recalcar que los nodos cuenta (azules) no pueden ser nodos líderes de tarjetas (verdes). Por otro lado, los nodos líder tarjeta (verdes) siempre son suscriptores a la tarjeta que lideran (rojos); más aún, todos los nodos del sistema cumplen mínimamente con el rol de suscriptor.  
En resumen:

- los nodos cuenta y los nodos líder tarjeta ejecutan también la responsabilidad de nodos suscriptores,
- los nodos líder tarjeta son, en particular, suscriptores a la tarjeta que lideran (también pueden estar suscriptos a otras tarjetas)
- y los nodos cuenta no pueden ser nodos líder. Si un nodo líder tarjeta asume la responsabilidad de ser un nodo cuenta, entonces tiene que delegar la responsabilidad de líder tarjeta a otro nodo del clúster de suscriptores a la tarjeta; de donde surge una última regla:
- un clúster de nodos suscriptos a una tarjeta tiene que cumplir con una cantidad mínima. En caso de no hacerlo, se invita a un nodo del sistema a suscribirse a la tarjeta.  

Agrupando los niveles de clúster (y obviando los surtidores), la vista general de una posible configuración del sistema se ve de la siguiente forma:

![*Overview* del sistema distribuido global.](doc/informe/diagrams/distributed-system-overview.svg)

### Paseo por varios casos de uso

#### 1. *Un conductor usa su tarjeta por primera vez en el surtidor de una estación.*
1. El conductor le da su tarjeta al cajero, que usa la terminal de cobro de la columna del surtidor que usó para cargar nafta. El surtidor necesita saber si el cobro puede o no ser efectuado. Para ello revisa la información de la tarjeta, como no la tiene en guardada, la solicita. El mensaje utilizado para la solicitud es delegado al nodo central de la estación. En este punto ya nos encontramos en el sistema distribuido de estaciones.  
2. Una vez que el nodo estación recibe el mensaje con la solicitud de información de la tarjeta, envía el mensaje a sus estaciones vecinas, y así lo hacen estas últimas, propagando el mensaje como un *virus*. El mensaje que se propaga contiene, además de la solicitud en sí misma, las direcciones a las que ya se propagó; para evitar demasiados mensajes redundantes.  
Como esta es la primera vez que la tarjeta es utilizada, ningún nodo va a contestar con su información y por lo tanto el nodo de la estación original genera el registro de la tarjeta.  
En caso de que el mensaje llegue a un nodo cuenta al que le pertenece la tarjeta, el mismo puede rápidamente contestar si la tarjeta ya existe o no.  
3. Una vez generado el registro, se deben tener un mínimo de nodos suscriptos a la misma, un nodo cuenta líder y un nodo cuenta generado para la tarjeta. Como ningún nodo cuenta contestó, y no ningún otro nodo tenía la tarjeta, se generan ambos. Además se invitan a la lista de suscripción al top $N$ nodos más cercanos para replicar en ellos la información del registro de la misma, y también porque el sistema no acepta un nodo que sea cuenta y líder tarjeta en simultáneo.  
4. Con todas las condiciones del sistema distribuido en orden, la estación procede a realizar el cobro para luego actualizar a los suscriptores de la tarjeta (que acaban de generarse).

#### 2. *Un conductor usa su tarjeta en el surtidor de una estación a la que frecuenta.*
Si un conductor utiliza su tarjeta en una estación a la que va con frecuencia, entonces ésta estación ya tiene cargado el registro de la tarjeta. Aún así, se necesita saber si a la cuenta le queda monto para realizar el cobro, para esto se procede de la siguiente manera:

1. El surtidor envía la consulta de saldo de cuenta al nodo líder de la estación.
2. El nodo líder de la estación envía la consulta de saldo de cuenta al nodo líder tarjeta.
3. El nodo líder tarjeta envía la consulta al nodo cuenta.
4. El nodo cuenta consulta las actualizaciones de los nodos líder del resto de tarjetas, computa la respuesta y se la envía al nodo líder tarjeta que le hizo la consulta.

#### 3. *Un conductor usa su tarjeta en una nueva estación nueva, habiéndola usado en otras.*
Si un conductor usa su tarjeta en una nueva estación, es decir, en una estación en la que todavía no la había usado, entonces la estación no va a contar con el registro de la tarjeta y por tanto propagará la consulta como en el caso 1. Ésta vez si va a recibir una respuesta de una de los nodos que estén suscriptos a la tarjeta, por lo que

1. gestiona el cobro como en 2,
2. envía el mensaje de *suscripción*,
3. invita a sus nodos cercanos,
4. y actualiza a la lista de nodos suscriptos por el cobro realizado.

### *Time-to-leave* (TTL)
Supongamos que un conductor utiliza siempre su tarjeta en las estaciones cercanas a su casa en Córdoba. Si el conductor se va, de manera espontánea, de viaje a Formosa (por trabajo, si no no usaría la tarjeta de la empresa...), entonces probablemente utilice varias estaciones entre Córdoba y Formosa. Cuando vuelva de de su jornada laboral (o de sus vacaciones si no hizo un buen uso de la tarjeta), no volvería a usar su tarjeta en las estaciones en las que la usó para viajar a Formosa.  
Sería un desperdicio de recursos—mínimos en memoria, pero sí significativos para la comunicación en la red—tener un nodo suscrito a la lista de una tarjeta si éste no fuera a volver a ser utilizado.  
Por esto se introduce el campo **TTL** en los registros de las tarjetas. Si un nodo es actualizado de manera *externa*, es decir, se actualiza la información de un registro de una de sus tarjetas sin que la tarjeta haya efectuado la carga en esa estación; un número mayor a TTL veces, entonces se elimina de la lista de suscripción de la tarjeta. De esta forma, evitamos que con el paso del tiempo el sistema gaste recursos actualizando a estaciones a las que no les debería importar el registro de una tarjeta.

### *Mutual exclusion*
Dado que las cuentas pertenecen a conductores y que éstos son entes físicos en el mundo real, se hace imposible que la misma tarjeta aparezca en dos estaciones distintas a la vez. Sin embargo, un caso de acceso mutable compartido que no queda descartado es el de la actualización del monto disponible en una cuenta. Gracias a la centralización de la comunicación cuenta-tarjeta por medio de líder tarjeta y siendo que es un único nodo, el nodo cuenta, quien sincrónica y atómicamente resuelve las consultas de saldo disponible; queda resuleto el problema de las *race condition* para este monto.

### *Node failure recovery*
Hasta ahora sólo consideramos los casos felices del funcionamiento del sistema, pero en la realidad los nodos pueden fallar. A continuación detallamos lo que pasaría en caso de que cada uno de los distintos tipos de nodos falle, a partir de la siguiente configuración arbitraria del sistema:

![Estado inicial del sistema. Dos cuentas, una con las tarjetas $T_1$ y $T_2$ y la otra con $T_3$, $T_4$ y $T_5$.](doc/informe/diagrams/recovery-initial-state.svg)

#### *Se cae $N_1$: nodo suscriptor.*
Que se caiga un nodo suscriptor no representa un problema demasiado grande. En este caso la estación va a tener que guardarse las actualizaciones a la tarjeta, sin poder realizar las consultas de suficiencia de saldo en las mismas o en sus cuentas. No hay nada más que hacer puesto que la única responsabilidad del nodo suscriptor es comunicar al nodo líder y no hay nunca posibilidad de que esto así ocurra.  
Cuando el nodo vuelve a la vida, tiene que preguntar quién es el leader, enviarle sus actualizaciones de la tarjeta a la que el clúster suscribe para que este actualice al resto de nodos en el clúster y al nodo recuperado, a este último con la agregación de las actualizaciones que acaba de enviar y las que se efectuaron durante su baja.

![*Recovery* de la falla en $N_1$.](doc/informe/diagrams/recovery-from-n1-failure.svg)

#### *Se cae $N_{13}$: nodo líder tarjeta.*
Que se caiga un nodo líder tarjeta representa un mayor problema ya que su responsabilidad es la de centralizar la información generada por un clúster sobre una tarjeta y estar disponible para cuando el nodo cuenta al que pertenece la tarjeta consulte la información de la misma. En este caso se usa el algoritmo de elección de líder *Bully* y se comunica al nodo cuenta sobre el líder elegido.  
En caso de que sea el nodo cuenta quien se entera de la baja del nodo líder, simplemente envía un mensaje de elección de líder, sin participar de la elección, y recibir el líder elegido al final de la misma.

![*Recovery* de la falla en $N_{13}$.](doc/informe/diagrams/recovery-from-n13-failure.svg)

#### *Se cae $N_{22}$: nodo cuenta.*
Este es el caso más complicado, ya que el nodo cuenta es el tipo de nodo con mayor responsabilidad del sistema. La dinámica de recovery de este caso es muy similar a la de cuando se cae un nodo líder tarjeta, sumando una re-elección del nodo líder tarjeta ya que las responsabilidades líder tarjeta y cuenta no son compatibles.

![*Recovery* de la falla en $N_{22}$. En este diagrama se obvia el algoritmo *bully* para elegir nodo líder del clúster suscripto a la tarjeta $T_5$ puesto que ya se mostró en mayor detalle en el caso anterior. (4.) Existen optimizaciones como hacer que $N_{21}$ mande un sólo mensaje de ELECTION a los nodos del clúster, pero en sí la idea es logar que los nodos elijan a un nuevo líder, ya que los nodos cuenta no pueden ser nodos líder tarjeta. (5.) Notar además que es $N_{21}$ quien se encarga de poner al clúster de suscriptores $T_5$ en modo elección, para no quedar elegido, siendo que es el de mayor ID, puede simplemente no contestar, o contestar con mensaje del tipo CANNOT.](doc/informe/diagrams/recovery-from-n22-failure.svg)

### Política de cobro en estaciones sin conexión
Dado que el sistema está implementado de manera distribuida, se puede dar el caso de que un conductor intente utilizar su tarjeta en una estación sin conexión. Parece más grave dejar a un conductor sin combustible a las tres de la mañana en la ruta que dejar pasar el límite con esa excepción. Quien asuma el gasto se va del scope del sistema.  
Se decidió manejar este problema de la siguiente manera: si una estación pierde la conexión y necesita realizar el cobro de una tarjeta entonces la realiza sin necesidad de checkear el saldo disponible, realiza el cobro y encola la actualización del registro (mensaje `UPDATE`) para cuando vuelva a tener conexión; agregando además el detalle de la transacción (en que estación se efectuó y en qué momento), de manera tal que se pueda configurar la política adoptada por el nodo líder que reciba la actualización cuando el nodo que la efectuó vuelva a funcionar. 

## Flujo de las consultas de los clientes
Cuando un **administrador** hace una consulta o impone un nuevo monto límite, ya sea de su cuenta principal o de una sus tarjetas, el mensaje de la consulta se direcciona a al nodo más cercano geográficamente. El nodo del server que recibe la consulta, checkea si tiene o no el registro y si no lo tiene propaga el mensaje de la misma forma en la que lo haría si se tratara de una consulta de registro entre nodos. Cuando llega el registro, el nodo que recibió la consulta del cliente, se suscribe al mismo y contesta al administrador, de esta manera ninguna estación está sobrecargada con peticiones de los clientes y las posteriores consultas se responden más rápido.

## Modelo de Actores

El sistema se modela siguiendo el **paradigma de actores distribuidos**. Cada nodo en el sistema distribuido ejecuta varios actores, con distintas responsabilidades. En particular si un nodo ejecuta, por ejemplo, un actor **Cuenta**, entonces, ese nodo es un **Nodo Cuenta**. Se mantiene la limitación de que un nodo cuenta no puede ser un nodo líder de una tarjeta que pertenece a esa cuenta, por tanto un nodo no puede tener actores cuenta 1 y lider de una tarjeta que pertenezca a la cuenta 1.  
Los ejecutables de los nodos proveen una abstracción de comunicación entre actores, de manera tal que un actor se comunica con otro como si ambos vivieran en el mismo nodo. Se separan entonces la comunicación entre actores en los nodos y entre los nodos en sí. Esto es porque los actores no engloban la responsabilidad de, por ejemplo, mantener una mínima cantidad de réplicas de una tarjeta en distintos nodos del sistema distribuído; los mensajes que pertenecen a esa coordinación se manejan en otro grupo de entidades que se comunican: los nodos.  
Cada actor mantiene su propio estado interno y procesa mensajes de forma asíncrona, garantizando independencia y resiliencia ante fallos.  
En este modelo se especifica el funcionamiento de la comunicación entre actores en los nodos del sistema y no la comunicación entre nodos, que ya se mostró en la sección de arquitectura del servidor.

![Diagrama del modelo de actores. Las entidades de la lógica de negocio del sistema están abstraídas del funcionamiento del sistema distribuido.](doc/informe/diagrams/actor-model-diagram.svg)

### Actores
- **Suscriptor**: mantiene registros locales de tarjetas que se usaron en su zona. Si no conoce una tarjeta, propaga el intento de cobro a sus estaciones vecinas. Además, valida el **límite de la tarjeta** antes de delegar el cobro al nodo líder.  
- **Líder Tarjeta**: lidera un clúster de suscriptores de una tarjeta. Recibe cobros de los suscriptores y los reenvía al nodo cuenta para su validación global, esto es, checkear el límite de la cuenta. Luego distribuye las actualizaciones a los nodos suscriptores.  
- **Cuenta**: centraliza el control del saldo total de la cuenta principal. Valida los límites globales de la empresa y confirma o rechaza las las operaciones.

Cada cuenta tiene un único nodo cuenta y cada tarjeta tiene un único **nodo líder**, que actúa como intermediario entre el nodo cuenta y los nodos suscriptores.  

### Mensajes del sistema

| Mensaje | Descripción |
|----------|-------------|
| **Cobrar** | Solicitud de cobro iniciada por un surtidor o reenviada entre nodos |
| **RespuestaCobro** | Confirmación o rechazo del cobro (por límite de tarjeta o de cuenta) |
| **Registro** | Información completa de una tarjeta, enviada cuando un nodo la conoce |
| **Actualización** | Propagada a todos los suscriptores después de un cobro exitoso |

### Representación en pseudocódigo Rust

```rust
enum ErrorCobro {
    // ...
}

// ======== Mensajes ========
enum Mensaje {
    // flujo principal
    Cobrar {
        // suscriptor -> líder tarjeta & líder tarjeta -> cuenta
        transaction_id: TransactionID,
        tarjeta: TarjetaID,
        monto: f64,
        origen: ActorID,
        destino: ActorID,
    },
    RespuestaCobro {
        // líder tarjeta -> suscriptor & cuenta -> líder tarjeta
        transaction_id: TransactionID,
        ok: bool,
        razon: ErrorCobro,
        monto: f64,
    },
    Actualizacion {
        // actualizaciones de límites de tarjetas/cuentas
    },
}

// ======== Estructura de los registros ========
struct RegistroTarjeta {
    tarjeta: TarjetaID,
    cuenta: CuentaID,
    saldo_usado: f64,
    limite_tarjeta: f64,
    ttl: u8,          // tiempo de vida de la suscripción
    lider_id: NodoID, // líder de la tarjeta
}

// ======== Suscriptor ========
struct Suscriptor {
    id: ActorID,
    tarjeta: RegistroTarjeta,
    pendientes: HashMap<TransactionID, f64>,
}

impl Suscriptor {
    // entrada del cobro en un surtidor a un suscriptor que contiene el nodo
    fn handle_cobrar(&mut self, transaction_id: TransactionID, tarjeta: TarjetaID, monto: f64) {
        tarjeta.ttl = INITIAL_TTL;
        if monto + tarjeta.gastado > tarjeta.limite {
            // no puedo cobrar
        }

        self.lider
            .send(Mensaje::Cobrar::new(transaction_id, tarjeta, monto));
    }

    fn handle_respuesta_cobro(
        &mut self,
        transaction_id: TransactionID,
        ok: bool,
        razon: String,
        monto: f64,
    ) {
        if ok {
            // se puede cargar nafta
            return;
        }

        // no se puede cargar nafta
    }

    // actualizar el registro de la tarjeta
    fn handle_actualizacion(&mut self, tarjeta: TarjetaID, monto: f64) {
        self.tarjeta.saldo_usado += monto;
        if transaction.origin != self {
            self.tarjeta.ttl -= 1;
            if self.tarjeta.ttl == 0 {
                self.tarjetas.remove(&tarjeta);
            }
        }
    }
}

// ======== Líder ========
struct Lider {
    id: ActorID,
    tarjeta: TarjetaID,
    cuenta: ActorID,
    suscriptores: Vec<ActorID>,
    pendientes: HashMap<TransactionID, (ActorID, f64)>,
}

impl Lider {
    // recibe Cobrar desde un suscriptor y lo reenvía a la cuenta
    fn handle_cobrar(
        &mut self,
        transaction_id: TransactionID,
        tarjeta: TarjetaID,
        monto: f64,
        origen: NodoID,
    ) {
        self.cuenta
            .send(Mensaje::Cobrar::new(transaction_id, tarjeta, monto));
    }

    // recibe la decisión de la cuenta
    fn handle_respuesta_cobro(
        &mut self,
        transaction_id: TransactionID,
        ok: bool,
        razon: String,
        monto: f64,
    ) {
        // responder al suscriptor que inició el flujo
        origen_suscriptor.enviar(
            origen_suscriptor,
            Mensaje::RespuestaCobro {
                transaction_id,
                ok,
                razon,
                monto: monto_guardado,
            },
        );

        // actualizar al resto de suscriptores del clúster
        for s in &self.suscriptores {
            s.enviar(
                Mensaje::Actualizacion::new(
                    tarjeta: self.tarjeta.clone(),
                    delta: monto_guardado,
                )
            );
        }
    }
}

// ======== Cuenta ========
struct Cuenta {
    id: NodoID,
    cuenta: CuentaID,
    limite: f64,
    consumo_total: f64,
}

impl Cuenta {
    // valida el límite global de la cuenta y decide el cobro
    fn handle_cobrar(
        &mut self,
        transaction_id: TransactionID,
        _tarjeta: TarjetaID,
        monto: f64,
        origen: ActorID, // líder
    ) {
        if self.consumo_total + monto >= self.limite {
            origen.enviar(
                Mensaje::RespuestaCobro::new(
                    transaction_id,
                    ok: false,
                    razon: ErrorCobro::LimiteCuenta;
                    monto,
                )
            );

            return;
        }

        self.consumo_total += monto;
        origen.enviar(
            Mensaje::RespuestaCobro::new(
                transaction_id,
                ok: true,
                razon: "".into(),
                monto,
            )
        );
    }
}
```

## Protocolo de comunicación
Por tratarse de un sistema distribuido, los nodos obviamente no comparten memoria, si no que se comunican por red. Es por esto que se hace necesario introducir un protocolo de aplicación y el elegir un protocolo de capa de transporte.

### Protocolo de capa de aplicación
Si bien los nodos tienen acceso al código de la implementación de las entidades del sistema, no comparten memoria, si no que se comunican enviando mensajes por red, y por tanto se hace necesario introducir un **protocolo** de *serialización* y *deserialización* de las tiras de bytes que se envían.  
El protocolo es simple, todos los mensajes tienen 1 byte para el tipo de mensaje (disponibilidad para $2^{1\times 8}=256$ tipos de mensaje distintos), de manera tal que el resto de la deserialización se lleva a cabo según este tipo. Además, cuando un actor quiere comunicarse con otro actor, delega esta comunicación a la abstracción de envío de mensajes entre actores en distintos nodos, por lo tanto todos los mensajes están *wrappeados* en un protocolo de más bajo nivel que contiene la información de la comunicación entre los nodos. Para los mensajes descriptos en la descripción del modelo de autores, se propone la siguiente estructura:

- **`Cobrar`.** `req_id`: ID de un actor, `tarjeta_id`: ID de una tarjeta, `monto`: doble precisión, `origen_id`: ID de un actor.
- **`RespuestaCobro`.** `req_id`: ID de un actor, `ok`: booleano, `razon`: enum de tipos de error, `monto`: doble precisión.
- **`RegistroCobro`.** `req_id`: ID de un actor, `registro`: *registro de tarjeta*.
- **`Actualización`.** `tarjeta`: ID de una tarjeta, `delta`: doble precisión.

El *registro de tarjeta* es **`RegistroTarjeta`:** `tarjeta`: ID de la tarjeta, `cuenta`: ID de la cuenta, `saldo_usado`: doble precisión, `limite_tarjeta`: doble precisión, `ttl`: entero positivo, `lider_id`: ID de un actor.  

Por último, los campos están definidos de la siguiente manera:

- **ID de una tarjeta.** No sabemos cuántas tarjetas hay, por lo que usamos 4 bytes para representar sus IDs ($2^{4\times 8}$, más de 4 mil millones de IDs distintos).
- **ID de un actor.** Debería haber más capacidad para actores que tarjetas en el sistema, por lo que se utilizan para definirlos 5 bytes.
- **Doble precisión.** Usamos el estándar 754 de la IEEE de doble precisión para todos los números que representan montos y fracciones de tiempo. Son 8 bytes: 1 bit de signo, 11 bits para el exponente y el resto de los 52 bits para la mantisa. En Rust esto es un `f64`.
- **Entero positivo.** Si sólo se usase para el TTL entonces bastaría con tener un byte para este campo.
- **Enum de tipos de error.** Un sólo byte para poder representar hasta 256 tipos de erorres distintos. Luego durante la deserialización debería traducirse el tipo a un mensaje legible por el usuario (si es que no se trata de un error del sistema que pueda ser manejable por el mismo).

### Wrapper de mensajes entre nodos
Los headers de los mensajes a nivel sistema distribuido, es decir, comunicación entre nodos que intercomunican a los actores que ejecutan tienen la siguiente estructura:

- **ID del nodo origen**: para identificar la dirección IP del de la estación involucrada en el mensaje. Sabemos que hay 1600 estaciones, por lo que bastarían 11 bits; para tener espacio para poder escalar a más estaciones se usan 2 bytes.
- **ID del nodo destino**.

Se podrían aceptar además campos del estilo **last-hop**, donde este campo serviría para definir el último interlocutor de la comunicación y no el origen de la misma, para realizar optimizaciones en la comunicación entre nodos.  

### Protocolo de capa de transporte
En el sistema **YPF Ruta** se utiliza el protocolo **TCP (Transmission Control Protocol)** tanto para la comunicación local entre *surtidores*, como para la comunicación entre los distintos nodos distribuidos del sistema (*suscriptores, líderes y cuentas*).  
TCP garantiza la **entrega confiable y ordenada** de los mensajes, propiedad esencial en un entorno donde cada operación representa una transacción económica. Además provee la detección de interrupciones de comunicación, que es esencial para que los nodos se enteren si sus pares fallan y actuen en concecuencia.

#### Comunicación local
Dentro de cada estación, los surtidores se conectan al nodo central mediante TCP sobre la red local (LAN).  
Este canal asegura que los mensajes `Cobrar` y las respuestas de autorización se transmitan sin pérdidas ni duplicaciones, manteniendo la coherencia del registro de ventas.

#### Comunicación entre nodos
Las estaciones y los distintos nodos del sistema intercambian información mediante TCP, manteniendo sincronizados los registros de tarjetas y cuentas.  
El uso de TCP facilita la detección de desconexiones, el control de flujo y la confirmación explícita de entrega, reduciendo la complejidad de los mecanismos de replicación y actualización distribuidos.
