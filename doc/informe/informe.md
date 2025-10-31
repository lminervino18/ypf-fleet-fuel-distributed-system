# YPF Ruta
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
Una forma de optimizar la comunicación entre nodos sería entonces tenerlos separados por cuentas: cada nodo tendría una réplica de la información de todas las tarjetas de la cuenta a la que pertenece y sólo debería comunicar a los otros nodos del clúster de la cuenta respecto de las actualizaciones de la misma. El problema con esto último es que una empresa grande, con muchas tarjetas y muchos conductores a lo largo del país; tendría réplicas innecesarias: un conductor que vive en Salta probablemente no use una estación en Santa Cruz, sin embargo, si uno de sus compañeros de trabajo así lo hace, entonces el registro de su tarjeta estaría replicado en la estación de Santa Cruz.  
La solución que se encontró es la de dividir los clústers por tarjeta y no por cuenta. Ahora bien, como también necesitamos centralizar la información de todas las tarjetas pertenecientes a una cuenta, surge la necesidad de los nodos *cuenta*. Para minimizar la comunicación de los nodos cuenta con los nodos de las tarjetas que le pertenecen, el rol de comunicador se centraliza en los nodos *líder tarjeta*.  

A continuación se explica más en profundidad cada uno de los tipos de clúster que se mencionaron.

#### Clúster de surtidores.

Los surtidores en una estación están conectados de manera local y se encargan de mantener actualizado al surtidor líder del clúster para que este ejerza la función de nodo estación en el sistema global.

\begin{figure}[H]
\centering
\includesvg[width=0.8\textwidth]{diagrams/station-cluster-overview}
\caption{Dos estaciones, con cuatro surtidores cada una.}
\end{figure}

#### Clúster de nodos suscriptos a una tarjeta.

Los nodos suscriptos a una tarjeta informan a sus pares de las actualizaciones en los registros de las tarjetas a las que suscriben. Hay un líder del clúster y los *súbditos* se encargan de elegirlo al principio de la ejecución y en caso de que el mismo deje de estar activo.

\begin{figure}[H]
\centering
\includesvg[width=0.8\textwidth]{diagrams/card-cluster-overview}
\caption{Clúster de nodos suscriptos a una tarjeta.}
\end{figure}

#### Clúster de cuenta.

El clúster de nodos líderes de tarjetas tienen su propio líder: el *nodo cuenta*. Dentro de éste clúster se mantiene actualizado al nodo cuenta ante cualquier cambio en alguno de los registros de las tarjetas que conforman la cuenta. Los *súbditos* eligen un líder al principio de la ejecución y en caso de que el mismo deje de estar activo. Las actualizaciones son comunicadas sólo cuando el nodo cuenta así lo solicita.

\begin{figure}[H]
\centering
\includesvg[width=0.8\textwidth]{diagrams/account-clusters-overview.svg}
\caption{Clúster de cuenta.}
\end{figure}

#### Vista de águila.

Cabe recalcar que los nodos cuenta (azules) no pueden ser nodos líderes de tarjetas (verdes). Por otro lado, los nodos líder tarjeta (verdes) siempre son suscriptores a la tarjeta que lideran (rojos); más aún, todos los nodos del sistema cumplen mínimamente con el rol de suscriptor.  
En resumen:

- los nodos cuenta y los nodos líder tarjeta ejecutan también la responsabilidad de nodos suscriptores,
- los nodos líder tarjeta son, en particular, suscriptores a la tarjeta que lideran (también pueden estar suscriptos a otras tarjetas)
- y los nodos cuenta no pueden ser nodos líder. Si un nodo líder tarjeta asume la responsabilidad de ser un nodo cuenta, entonces tiene que delegar la responsabilidad de líder tarjeta a otro nodo del clúster de suscriptores a la tarjeta; de donde surge una última regla:
- un clúster de nodos suscriptos a una tarjeta tiene que cumplir con una cantidad mínima. En caso de no hacerlo, se invita a un nodo del sistema a suscribirse a la tarjeta.  

Agrupando los niveles de clúster (y obviando los surtidores), la vista general de una posible configuración del sistema se ve de la siguiente forma:

\begin{figure}[H]
\centering
\includesvg[width=0.8\textwidth]{diagrams/distributed-system-overview.svg}
\caption{*Overview* del sistema distribuido global.}
\end{figure}

#### Paseo por varios casos de uso.

### 1. *Un conductor usa su tarjeta por primera vez en el surtidor de una estación*
El conductor le da su tarjeta al cajero, que usa la terminal de cobro de la columna del surtidor que usó para cargar nafta. El surtidor (a partir de este punto llamamos surtidor a la terminal de cobro del mismo) necesita saber si el cobro puede o no ser efectuado. Para ello, solicita la información de la tarjeta. El mensaje utilizado para la solicitud es delegado al nodo central de la estación. En este punto ya nos encontramos en el sistema distribuido de estaciones.  
Una vez que el nodo estación recibe el mensaje con la solicitud de información de la tarjeta, envía a sus estaciones (nodos) vecinas (definidas por distancia máxima) el mensaje, y así lo hacen estas últimas, propagando el mensaje como un *virus* // TODO: explicar el algoritmo que evita los mensajes repetidos de alguman manera (y encontrar ese algoritmo xd).  
Como esta es la primera vez que la tarjeta es utilizada, ningún nodo va a contestar con su información y por lo tanto el nodo de la estación original va a tener que generar el registro de la tarjeta. Una vez generado el registro, se *suscriben* a los nodos vecinos a la lista de actualización de la tarjeta.  
Los nodos pertenecientes a una lista de suscripción de una tarjeta, son informados por cualquier actualización de la misma. Cuando un nodo actualiza un registro, se fija a quiénes tiene que informar sobre esa actualización a partir de la lista de suscripción de ese registro.

Hasta ahora sólo hablamos de tarjetas y pasamos por alto las cuentas principales. Definimos un nodo **cuenta (principal)** a un nodo que tiene una lista de direcciones de otros nodos de manera tal que la unión de las suscripciones de esos otros nodos resulte en la totalidad de las tarjetas de la cuenta principal.  
Los nodos **estación líder** son los encargados de mantener actualidos a los nodos cuenta principal de las actualizaciones de una tarjeta. Es decir:

1. Cuando una tarjeta conocida por una estación es utilizada, la estación actualiza a todos aquellos nodos que formen parte de la lista de suscripción de la tarjeta.
2. Cuando al nodo estación líder le llega la actualización del registro, envía un mensaje al nodo cuenta principal.
3. El nodo cuenta simplemente almacena la actualización, **no** comunica al resto de clústers de nodos de otras tarjetas.
4. Cuando un nodo necesita consultar el límite de la cuenta principal asociada a una tarjeta, delega la consulta al nodo líder y éste hace la consulta al nodo cuenta principal. De esta manera, una tarjeta que se usa con muy poca frecuencia no necesita ser actualizada constantemente acerca de los registros de una tarjeta que se utiliza con mucha frecuencia y que pertenece a la misma cuenta principal.


### 2. *Un conductor usa su tarjeta en el surtidor de una estación a la que frecuenta*
Si un conductor utiliza su tarjeta en una estación a la que va con frecuencia, entonces esta estación, ya tiene cargado el registro de la tarjeta y simplemente necesita delegar la consulta de verificación de no-exceso de límite de cuenta al nodo tarjeta líder para que el mismo consulte al nodo cuenta y checkear si se puede o no hacer la transacción. Cuando tiene el OK, realiza la operación y actualiza a los nodos de la lista de suscripción de la tarjeta.

### 3. *Un conductor usa su tarjeta en una nueva estación nueva, habiéndola usado en otras*
Si un conductor, que ya usó su tarjeta anteriormente en otras estaciones, la utiliza en una estación en la que nunca la había utilizado, entonces:
1. El surtidor envía la consulta a la estación.
2. La estación propaga la consulta *virus* hacia sus nodos vecinos y estos a sus vecinos.
3. Eventualmente un nodo que tiene el registro de la tarjeta contesta la consulta con la información de la misma.
4. La estación que hizo la consulta, se agrega a la lista de suscripción de la tarjeta y procede como si fuera el caso 2.

TODO: resumen del sistema servidor distribuido.

### TTL
Supongamos que un conductor utiliza siempre su tarjeta en las estaciones cercanas a su casa en Córdoba. Si el conductor se va de viaje a Formosa (y por trabajo, si no no usaría la tarjeta de la empresa...), entonces probablemente utilice varias estaciones entre Córdoba y Formosa. Cuando vuelva de sus vacaciones, o de su jornada laboral, no volvería a usar su tarjeta en las estaciones en las que la usó para viajar a Formosa.  
Sería un desperdicio de recursos (espacialmente mínimos pero sí significativos para la comunicación en la red) tener un nodo suscrito a la lista de una tarjeta si éste no fuera a volver a ser utilizado. Por esto se introduce el campo **TTL** a los registros de las tarjetas.  
Si un nodo es actualizado de manera *externa*, es decir, se actualiza la información de un registro de una de sus tarjetas sin que la tarjeta haya efectuado la carga en esa estación; un número mayor a TTL veces, entonces se elimina de la lista de suscripción de la tarjeta.

## Flujo de las consultas de los clientes
- Cuando un **administrador** hace una consulta de ya sea su cuenta principal o una de sus tarjetas, propaga el mensaje desde el nodo más cercano hasta su ubicación. Cada uno de los nodos del server que recibe la consulta, checkea si tiene o no el registro de la tarjeta, si no la tiene propaga el mensaje. Eventualmente uno de los nodos que recibe la consulta contiene la información y le contesta al administrador, (TODO: ya sea directamente o por medio de un nodo que mantenga una pared entre cliente y los nodos de las estaciones).
- Para actualizar el límite de cuenta o de tarjeta, se procede como en el caso anterior sólo que ahora contestan nodos cuenta o nodos suscritos a la tarjeta, respectivamente.

## Modelo de actores
Como probablemente ya se haya inferido, el modelo que plantea el sistema es un modelo de actores, ejecutado sobre un sistema distribuido.

### Actores
Los actores del modelo son:
- **nodo estación suscriptor**: nodo estación que está suscripto a un registro de una tarjeta.
- **nodo estación lider**: nodo estación que está suscripto a un registro de una tarjeta y que tiene como responsabilidad mantener actualizado al nodo cuenta.
- **nodo cuenta**: nodo que tiene direcciones de nodos estácion líder de manera tal que la unión de las suscripciones de esos nodos equivalga al conjunto de tarjetas de la cuenta.
- **nodo surtidor**: nodo que se ejecuta en la red local de una estación y que sólo realiza consultas al nodo de la estación.

Los actores estación suscriptor, estación lider y estación cuenta pueden correrse sobre uno o distintos nodos, idealmente, y como consecuencia de los algoritmos de actualización del sistema, *un nodo cuenta siempre va a ejecutarse en el mismo nodo que un nodo estación líder*.

### Mensajes
- **Cobrar**: el mensaje que envía el nodo surtidor al nodo central de su estación.
- **Consulta Registro**: el mensaje que se propaga como consecuencia de que una estación no conozca una tarjeta que le llega de uno de sus surtidores.
- **Registro**: La contestación al mensaje anterior.
- **Suscripción**: El mensaje que un nodo envía a los suscriptores de una tarjeta para que lo agreguen a la lista de suscriptores en sus registros de esa tarjeta.
- **Actualización**: el mensaje que se propaga a los nodos suscriptos a una tarjeta cada vez que esta se actualiza en cualquiera de los nodos que están suscriptos a ellas.
- TODO: **Líder estación**: Mensaje que se propaga para elegir un líder en un clúster de nodos suscriptos a una tarjeta.
- TODO: **Líder cuenta**: Mensaje que se propaga para elegir un líder en un conjunto de clústers de nodos suscriptos a las tarjetas que componen una cuenta principal.
