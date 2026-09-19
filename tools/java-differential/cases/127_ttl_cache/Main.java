import java.util.HashMap;

public class Main {
    interface Clock {
        long now();
    }

    static class FakeClock implements Clock {
        private long t = 0;
        public long now() { return t; }
        void advance(long ms) { t += ms; }
    }

    static class Entry<K, V> {
        K key;
        V value;
        long expires;
        Entry<K, V> prev = null;
        Entry<K, V> next = null;

        Entry(K key, V value, long expires) {
            this.key = key;
            this.value = value;
            this.expires = expires;
        }
    }

    static class TtlCache<K, V> {
        private final int capacity;
        private final long ttl;
        private final Clock clock;
        private final HashMap<K, Entry<K, V>> map = new HashMap<>();
        private Entry<K, V> head = null;
        private Entry<K, V> tail = null;
        int hits = 0;
        int misses = 0;
        int expired = 0;
        int evicted = 0;

        TtlCache(int capacity, long ttl, Clock clock) {
            this.capacity = capacity;
            this.ttl = ttl;
            this.clock = clock;
        }

        private void unlink(Entry<K, V> e) {
            Entry<K, V> p = e.prev;
            Entry<K, V> n = e.next;
            if (p != null) { p.next = n; } else { head = n; }
            if (n != null) { n.prev = p; } else { tail = p; }
            e.prev = null;
            e.next = null;
        }

        private void pushFront(Entry<K, V> e) {
            e.next = head;
            if (head != null) { head.prev = e; }
            head = e;
            if (tail == null) { tail = e; }
        }

        V get(K key) {
            Entry<K, V> e = map.get(key);
            if (e == null) {
                misses++;
                return null;
            }
            if (clock.now() >= e.expires) {
                unlink(e);
                map.remove(key);
                expired++;
                misses++;
                return null;
            }
            unlink(e);
            pushFront(e);
            hits++;
            return e.value;
        }

        void put(K key, V value) {
            Entry<K, V> found = map.get(key);
            if (found != null) {
                found.value = value;
                found.expires = clock.now() + ttl;
                unlink(found);
                pushFront(found);
                return;
            }
            if (map.size() == capacity) {
                Entry<K, V> old = tail;
                if (old != null) {
                    unlink(old);
                    map.remove(old.key);
                    evicted++;
                }
            }
            Entry<K, V> e = new Entry<>(key, value, clock.now() + ttl);
            map.put(key, e);
            pushFront(e);
        }

        int size() { return map.size(); }

        String order() {
            String out = "";
            Entry<K, V> cur = head;
            while (cur != null) {
                out = out + cur.key + " ";
                cur = cur.next;
            }
            return out.trim();
        }
    }

    static String show(String v) {
        return v == null ? "-" : v;
    }

    public static void main(String[] args) {
        FakeClock clock = new FakeClock();
        TtlCache<String, String> cache = new TtlCache<>(3, 100, clock);

        cache.put("a", "apple");
        cache.put("b", "banana");
        cache.put("c", "cherry");
        System.out.println(cache.order());

        System.out.println(show(cache.get("a")));
        System.out.println(cache.order());

        cache.put("d", "date");
        System.out.println(cache.order() + " | size " + cache.size());
        System.out.println(show(cache.get("b")));

        clock.advance(60);
        cache.put("a", "apricot");
        clock.advance(60);
        System.out.println(show(cache.get("c")));
        System.out.println(show(cache.get("a")));
        System.out.println(cache.order());

        clock.advance(200);
        System.out.println(show(cache.get("a")));
        System.out.println("hits " + cache.hits + " misses " + cache.misses + " expired " + cache.expired + " evicted " + cache.evicted);

        TtlCache<Integer, Long> counts = new TtlCache<>(2, 50, clock);
        counts.put(1, 10L);
        counts.put(2, 20L);
        counts.put(3, 30L);
        Long one = counts.get(1);
        Long three = counts.get(3);
        System.out.println((one == null ? "gone" : "kept") + " " + (three == null ? 0L : three));
    }
}
