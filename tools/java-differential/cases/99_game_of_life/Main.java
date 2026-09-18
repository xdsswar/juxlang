public class Main {
    static class Life {
        private int rows;
        private int cols;
        private boolean[][] cells;

        Life(int rows, int cols) {
            this.rows = rows;
            this.cols = cols;
            this.cells = new boolean[rows][cols];
        }

        void set(int r, int c) { cells[r][c] = true; }

        private int neighbours(int r, int c) {
            int n = 0;
            for (int dr = -1; dr <= 1; dr++) {
                for (int dc = -1; dc <= 1; dc++) {
                    if (dr == 0 && dc == 0) { continue; }
                    int rr = (r + dr + rows) % rows;
                    int cc = (c + dc + cols) % cols;
                    if (cells[rr][cc]) { n++; }
                }
            }
            return n;
        }

        void step() {
            boolean[][] next = new boolean[rows][cols];
            for (int r = 0; r < rows; r++) {
                for (int c = 0; c < cols; c++) {
                    int n = neighbours(r, c);
                    next[r][c] = cells[r][c] ? (n == 2 || n == 3) : n == 3;
                }
            }
            cells = next;
        }

        int population() {
            int p = 0;
            for (boolean[] row : cells) {
                for (boolean alive : row) {
                    if (alive) { p++; }
                }
            }
            return p;
        }

        String show() {
            String out = "";
            for (int r = 0; r < rows; r++) {
                for (int c = 0; c < cols; c++) {
                    out = out + (cells[r][c] ? "#" : ".");
                }
                if (r < rows - 1) { out = out + "\n"; }
            }
            return out;
        }
    }

    public static void main(String[] args) {
        Life life = new Life(6, 6);
        life.set(0, 1);
        life.set(1, 2);
        life.set(2, 0);
        life.set(2, 1);
        life.set(2, 2);
        System.out.println(life.show());
        String history = "";
        for (int g = 0; g < 8; g++) {
            history = history + life.population() + " ";
            life.step();
        }
        System.out.println(history.trim());
        System.out.println(life.show());
    }
}
