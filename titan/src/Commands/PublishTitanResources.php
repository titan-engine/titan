<?php

namespace Titan\Commands;

use Illuminate\Console\Command;

class PublishTitanResources extends Command
{
    /**
     * The name and signature of the console command.
     *
     * @var string
     */
    protected $signature = 'titan:resources';

    /**
     * The console command description.
     *
     * @var string
     */
    protected $description = 'Publish Titans resources';

    /**
     * Create a new command instance.
     *
     * @return void
     */
    public function __construct()
    {
        parent::__construct();
    }

    /**
     * Execute the console command.
     *
     * @return mixed
     */
    public function handle()
    {
        \Artisan::call('vendor:publish', [
            '--force' => true,
            '--tag' => 'titan'
        ]);

        $theme_path = base_path('themes');
        if (\File::copyDirectory($theme_path, storage_path('themes'))) {
            \Artisan::call('theme:install', [
                'package' => 'Default'
            ]);
            \Artisan::call('theme:install', [
                'package' => 'Admin'
            ]);
        } else {
            $this->error("Failed to copy themes from " . $theme_path);
        }
    }
}
