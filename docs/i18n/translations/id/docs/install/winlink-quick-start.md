# Linkwise dengan Winlink Express

Wisaya Windows mendukung audio bawaan dan mitra bentuk gelombang tanpa radio. Untuk audio bawaan, pilih lebar pita lalu masukkan nama perangkat perekaman dan pemutaran persis seperti keluaran `linkwise-modemd --audio-devices`. Di Express, gunakan porta perintah dan lebar pita yang sama dengan wisaya. Lihat [panduan pengaturan stasiun](../../../../../../rust/docs/modem-setup.md#run-the-two-way-modem) untuk perutean audio dan kendali radio.

Untuk pengujian BW500 tanpa radio, buat profil pengukuran dengan `python tools/fetch-modem-profile.py` dari repositori Linkwise.

1. Buka **Configure Linkwise for Winlink**. Isi tanda panggil yang sama dengan Express. Pilih **Radio-free waveform test** dan profil JSON BW500, lalu klik **Apply and start service**.
2. Gunakan porta perintah **8300**, atau **18310** bila 8300 sudah dipakai VARA asli. Linkwise menampilkan porta DATA dan porta mitra bentuk gelombang. Keduanya adalah dua porta berikutnya setelah porta perintah.
3. Sambungkan mitra bentuk gelombang ke porta yang ditampilkan. Layanan menerima satu mitra lokal. Jembatan opsional `linkwise-modemd --virtual-audio-device` memerlukan jalur masukan dan keluaran virtual yang terpisah. Jalankan dari desktop Windows jika pengandar audio memerlukan sesi masuk.
4. Di Express, buka **VARA HF P2P**, lalu **Settings → TNC Setup**. Masukkan **127.0.0.1** dan porta perintah dari wisaya. Express menentukan porta DATA. Pilih lebar pita maksimum **500** dan nonaktifkan **Automatically launch TNC when session is opened**. Simpan lalu buka kembali sesi.
5. Ketika Express menunjukkan **Ready**, masukkan tanda panggil mitra terisolasi dan frekuensi tengah uji. Masukkan pesan P2P dengan lampiran sintetis ke antrean. Periksa SHA-256 lampiran di pihak penerima dan pastikan kedua log sesi mencatat pemutusan sambungan.

Jika Express gagal terhubung, periksa alamat dan porta perintah. Di Event Viewer, buka **Windows Logs → Application** dan filter dengan `LinkwiseModem`. Jika terhubung tetapi belum Ready, periksa sambungan mitra bentuk gelombang serta kesesuaian tanda panggil dan lebar pita.
