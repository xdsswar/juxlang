import java.util.ArrayDeque;
import java.util.ArrayList;

public class Main {
    record Edge(int to, int weight) {}

    static class Graph {
        int n;
        ArrayList<ArrayList<Edge>> adj = new ArrayList<>();

        Graph(int n) {
            this.n = n;
            for (int i = 0; i < n; i++) {
                adj.add(new ArrayList<>());
            }
        }

        void add(int from, int to, int weight) {
            adj.get(from).add(new Edge(to, weight));
        }

        void addBoth(int a, int b, int weight) {
            add(a, b, weight);
            add(b, a, weight);
        }
    }

    static int[] bfs(Graph g, int start) {
        int[] dist = new int[g.n];
        for (int i = 0; i < g.n; i++) {
            dist[i] = -1;
        }
        ArrayDeque<Integer> queue = new ArrayDeque<>();
        dist[start] = 0;
        queue.addLast(start);
        while (!queue.isEmpty()) {
            int at = queue.pollFirst();
            for (Edge e : g.adj.get(at)) {
                if (dist[e.to()] == -1) {
                    dist[e.to()] = dist[at] + 1;
                    queue.addLast(e.to());
                }
            }
        }
        return dist;
    }

    static class Topo {
        private Graph g;
        private int[] state;
        ArrayList<Integer> order = new ArrayList<>();
        boolean cyclic = false;

        Topo(Graph g) {
            this.g = g;
            this.state = new int[g.n];
        }

        void visit(int v) {
            if (state[v] == 2) { return; }
            if (state[v] == 1) {
                cyclic = true;
                return;
            }
            state[v] = 1;
            for (Edge e : g.adj.get(v)) {
                visit(e.to());
            }
            state[v] = 2;
            order.add(v);
        }

        String run() {
            for (int v = 0; v < g.n; v++) {
                visit(v);
            }
            if (cyclic) { return "cycle"; }
            String out = "";
            for (int k = order.size(); k > 0; k--) {
                out += order.get(k - 1) + " ";
            }
            return out.trim();
        }
    }

    static long[] dijkstra(Graph g, int source) {
        final long inf = 1000000000L;
        long[] dist = new long[g.n];
        boolean[] done = new boolean[g.n];
        for (int i = 0; i < g.n; i++) {
            dist[i] = inf;
        }
        dist[source] = 0;
        for (int round = 0; round < g.n; round++) {
            int best = -1;
            for (int v = 0; v < g.n; v++) {
                if (!done[v] && dist[v] < inf && (best == -1 || dist[v] < dist[best])) {
                    best = v;
                }
            }
            if (best == -1) { break; }
            done[best] = true;
            for (Edge e : g.adj.get(best)) {
                long cand = dist[best] + e.weight();
                if (cand < dist[e.to()]) {
                    dist[e.to()] = cand;
                }
            }
        }
        return dist;
    }

    static int components(Graph g) {
        boolean[] seen = new boolean[g.n];
        int count = 0;
        for (int s = 0; s < g.n; s++) {
            if (seen[s]) { continue; }
            count++;
            ArrayList<Integer> stack = new ArrayList<>();
            stack.add(s);
            seen[s] = true;
            while (stack.size() > 0) {
                int v = stack.remove(stack.size() - 1);
                for (Edge e : g.adj.get(v)) {
                    if (!seen[e.to()]) {
                        seen[e.to()] = true;
                        stack.add(e.to());
                    }
                }
            }
        }
        return count;
    }

    static String joinInts(int[] xs) {
        String out = "";
        for (int i = 0; i < xs.length; i++) {
            if (i > 0) { out += " "; }
            out += xs[i];
        }
        return out;
    }

    public static void main(String[] args) {
        Graph roads = new Graph(7);
        roads.addBoth(0, 1, 7);
        roads.addBoth(0, 2, 9);
        roads.addBoth(0, 5, 14);
        roads.addBoth(1, 2, 10);
        roads.addBoth(1, 3, 15);
        roads.addBoth(2, 3, 11);
        roads.addBoth(2, 5, 2);
        roads.addBoth(3, 4, 6);
        roads.addBoth(4, 5, 9);
        System.out.println("bfs from 0: " + joinInts(bfs(roads, 0)));
        long[] d = dijkstra(roads, 0);
        String line = "";
        for (int i = 0; i < d.length; i++) {
            line += i + "=" + (d[i] == 1000000000L ? "inf" : "" + d[i]) + " ";
        }
        System.out.println("dijkstra: " + line.trim());
        System.out.println("components: " + components(roads));

        Graph tasks = new Graph(6);
        tasks.add(5, 2, 1);
        tasks.add(5, 0, 1);
        tasks.add(4, 0, 1);
        tasks.add(4, 1, 1);
        tasks.add(2, 3, 1);
        tasks.add(3, 1, 1);
        System.out.println("topo: " + new Topo(tasks).run());
        tasks.add(1, 5, 1);
        System.out.println("topo with cycle: " + new Topo(tasks).run());
    }
}
