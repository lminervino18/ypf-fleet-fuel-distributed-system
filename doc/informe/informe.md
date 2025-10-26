# YPF Ruta
## Aplicaciones
### Server
La arquitectura del server del sistema está implementada de manera distribuida. Cada nodo, ejecuta uno de los siguientes aplicaciones:

- Estación
- Surtidor

### Cliente
El único cliente (fuera del servidor de YPF) es el **administrador**. El administrador es responsables de

- limitar los montos disponibles tanto en las cuentas principales,
- como en las tarjetas. Y de
- consultar los saldos de las cuentas
- y de las tarjetas. Además, puede
- solicitar las facturación de las cuenta principal que le pertenece.

## Arquitectura del servidor
Como ya se mencionó, el servidor está compuesto de dos ejecutables: **estación** y **surtidor**. Éstas entidades componen dos sistemas distribuidos separados: el sistema de estaciones, que contiene la información de todas las cuentas y de todas sus tarjetas, y el sistema de surtidores de cada estación, que mantiene la información que se genera de las tarjetas en cada surtidor, quedando centralizada en el nodo principal de la estación.  

En el sistema de estaciones, cada estación es un nodo. Para demostrar el funcionamiento de este sistema, vamos a hacer un recorrido de las situaciones en las que puede encontrarse:

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
